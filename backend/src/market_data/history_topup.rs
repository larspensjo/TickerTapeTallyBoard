//! Repair long-history needs after the ordinary latest refresh has published prices.
use std::{collections::BTreeMap, sync::atomic::Ordering};

use chrono::{DateTime, Duration, NaiveDate, Utc};
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;

use crate::{
    clock::Clock,
    db::{
        fundamental_values, history_topups, market_data_runs, prices, provider_symbols,
        transactions, RepoError,
    },
    domain::{
        opportunity_evidence::{
            lookback_series::{prepare_lookback, HistoryClose, HistoryTopUpNeed, LookbackSeries},
            parameters::LOOKBACK_DAYS,
        },
        ProviderCode, RecentWindow, TransactionKind,
    },
    providers::{MarketDataProvider, PriceHistoryRequest, ProviderError, ProviderMissingReason},
};

use super::{
    effective_prices,
    price_source_refresh::provider_error_status,
    provider_registry::ProviderSet,
    refresh::{
        position_for_instrument, MarketDataError, RefreshItem, RefreshItemKind, RefreshItemStatus,
        RefreshLease, RefreshTarget,
    },
};

/// Boundaries are observed closes, never a moving look-back start or close count.
#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum RemainingNeed {
    Short,
    LeadingGap { first: NaiveDate },
    Gap { left: NaiveDate, right: NaiveDate },
    SplitStale { date: NaiveDate },
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct NeedSnapshot {
    version: u8,
    symbol: String,
    asset_class: Option<String>,
    quote_currency: Option<String>,
    instrument_currency: String,
    needs: Vec<RemainingNeed>,
}

impl NeedSnapshot {
    fn new(
        mapping: &provider_symbols::ProviderSymbolRow,
        currency: &str,
        series: &LookbackSeries,
    ) -> Self {
        let needs = series
            .top_up_needs()
            .into_iter()
            .filter_map(|need| match need {
                HistoryTopUpNeed::Short { .. } => Some(RemainingNeed::Short),
                HistoryTopUpNeed::Gap { start } => {
                    let right = series.closes.iter().find(|close| close.date > start)?.date;
                    if start == series.start
                        && series
                            .closes
                            .first()
                            .is_some_and(|close| close.date > start)
                    {
                        Some(RemainingNeed::LeadingGap { first: right })
                    } else {
                        Some(RemainingNeed::Gap { left: start, right })
                    }
                }
                HistoryTopUpNeed::SplitStale { date } => Some(RemainingNeed::SplitStale { date }),
                HistoryTopUpNeed::SplitAdjustmentUnverified { .. } => None,
            })
            .collect();
        Self {
            version: 1,
            symbol: mapping.provider_symbol.clone(),
            asset_class: mapping.asset_class.clone(),
            quote_currency: mapping.currency.clone(),
            instrument_currency: currency.to_owned(),
            needs,
        }
    }

    fn covered_by(&self, old: &Self, lookback_start: NaiveDate) -> bool {
        self.symbol == old.symbol
            && self.asset_class == old.asset_class
            && self.quote_currency == old.quote_currency
            && self.instrument_currency == old.instrument_currency
            && self.needs.iter().all(|need| {
                old.needs.iter().any(|previous| match (need, previous) {
                    (RemainingNeed::Short, RemainingNeed::Short) => true,
                    (
                        RemainingNeed::SplitStale { date },
                        RemainingNeed::SplitStale { date: old_date },
                    ) => date == old_date,
                    (
                        RemainingNeed::LeadingGap { first },
                        RemainingNeed::LeadingGap { first: old_first },
                    ) => first <= old_first,
                    (RemainingNeed::Gap { right, .. }, RemainingNeed::LeadingGap { first }) => {
                        right <= first
                    }
                    (RemainingNeed::LeadingGap { first }, RemainingNeed::Gap { left, right }) => {
                        lookback_start >= *left && first <= right
                    }
                    (
                        RemainingNeed::Gap { left, right },
                        RemainingNeed::Gap {
                            left: old_left,
                            right: old_right,
                        },
                    ) => left >= old_left && right <= old_right,
                    _ => false,
                })
            })
    }
}

/// One range is shared by requests, reservations, and diagnostic context.
fn request_range(today: NaiveDate) -> (NaiveDate, NaiveDate) {
    (
        today - Duration::days(LOOKBACK_DAYS + RecentWindow::ThirtyDays.days()),
        today,
    )
}

#[derive(Clone, Copy)]
struct HistoryContext<'a> {
    lease: &'a RefreshLease,
    target: &'a RefreshTarget,
    provider: Option<MarketDataProvider>,
    symbol: &'a str,
    start: NaiveDate,
    end: NaiveDate,
}

impl HistoryContext<'_> {
    fn error(&self, operation: &str, error: impl Into<MarketDataError>) -> MarketDataError {
        let error = error.into();
        crate::engine_warn!("history top-up failure run_id={} instrument_id={} provider={} symbol={} start={} end={} operation={} error={}", self.lease.run_id, self.target.instrument.id, self.provider.map_or("none", |provider| provider.as_str()), self.symbol, self.start, self.end, operation, error);
        error
    }
}

/// A provider stop applies only to this refresh, including its later index pass.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum PassOutcome {
    Completed,
    ProviderStopped,
}

pub(super) async fn run(
    providers: &ProviderSet,
    pool: &SqlitePool,
    targets: &[RefreshTarget],
    grouped: &BTreeMap<i64, Vec<transactions::TransactionRow>>,
    clock: &Clock,
    lease: &RefreshLease,
    items: &mut Vec<RefreshItem>,
) -> Result<PassOutcome, MarketDataError> {
    let (start, today) = request_range(clock.today());
    for target in targets {
        let context = HistoryContext {
            lease,
            target,
            provider: None,
            symbol: &target.instrument.symbol,
            start,
            end: today,
        };
        check_cancelled(lease)?;
        if position_for_instrument(grouped, target.instrument.id)
            .map_err(|error| context.error("derive_position", error))?
            .quantity
            <= 0
        {
            continue;
        }
        // Select the instrument's latest source once; never join different feeds into history.
        let latest =
            effective_prices::effective_latest_on_or_before(pool, target.instrument.id, today)
                .await
                .map_err(|error| context.error("effective_latest", error))?;
        let sources = effective_prices::enabled_price_sources(pool, target.instrument.id)
            .await
            .map_err(|error| context.error("enabled_sources", error))?;
        let selected = latest
            .as_ref()
            .map(|close| close.source.clone())
            .or_else(|| {
                sources
                    .first()
                    .map(|source| ProviderCode::new(source.provider.as_str()))
            });
        for source in sources
            .into_iter()
            .filter(|source| Some(ProviderCode::new(source.provider.as_str())) == selected)
        {
            let context = HistoryContext {
                provider: Some(source.provider),
                symbol: &source.provider_symbol,
                ..context
            };
            let Some(mapping) = provider_symbols::find_by_instrument_provider(
                pool,
                target.instrument.id,
                source.provider,
            )
            .await
            .map_err(|error| context.error("mapping_lookup", error))?
            else {
                continue;
            };
            if !mapping.enabled {
                continue;
            }
            let context = HistoryContext {
                provider: Some(mapping.provider),
                symbol: &mapping.provider_symbol,
                ..context
            };
            check_cancelled(lease)?;
            let mut stop_pass = false;
            match top_up_one(
                providers,
                pool,
                &mapping,
                grouped,
                clock,
                &context,
                &mut stop_pass,
            )
            .await
            {
                Ok(Some(item)) => {
                    items.push(item.history);
                    items.extend(item.dividend);
                }
                Ok(None) => {}
                Err(MarketDataError::LeaseLost) => return Err(MarketDataError::LeaseLost),
                Err(_) => {
                    // Logged at the failing operation with this instrument's context.
                    items.push(history_item(
                        target,
                        &mapping,
                        RefreshItemStatus::Failed,
                        Some("repository_error".to_owned()),
                        0,
                    ));
                }
            }
            // Stop on throttling or outage even if persisting its outcome failed.
            if stop_pass {
                return Ok(PassOutcome::ProviderStopped);
            }
        }
    }
    Ok(PassOutcome::Completed)
}

async fn load_series(
    pool: &SqlitePool,
    target: &RefreshTarget,
    mapping: &provider_symbols::ProviderSymbolRow,
    grouped: &BTreeMap<i64, Vec<transactions::TransactionRow>>,
    context: &HistoryContext<'_>,
) -> Result<LookbackSeries, MarketDataError> {
    let today = context.end;
    let stored = prices::list_for_instrument_in_range(
        pool,
        target.instrument.id,
        mapping.provider,
        Some(today - Duration::days(LOOKBACK_DAYS)),
        Some(today),
    )
    .await
    .map_err(|error| context.error("load_prices", error))?;
    let closes = stored
        .into_iter()
        .map(|row| {
            Ok(HistoryClose {
                date: row.date_value()?,
                close: row.close_decimal()?,
                currency: row.currency,
                provider: ProviderCode::new(row.provider.as_str()),
                fetched_at: DateTime::parse_from_rfc3339(&row.fetched_at)
                    .map_err(|error| RepoError::Decode(format!("price fetch timestamp: {error}")))?
                    .with_timezone(&Utc),
            })
        })
        .collect::<Result<Vec<_>, RepoError>>()
        .map_err(|error| context.error("decode_prices", error))?;
    let ledger_splits = grouped
        .get(&target.instrument.id)
        .into_iter()
        .flatten()
        .map(|row| row.to_ledger())
        .collect::<Result<Vec<_>, RepoError>>()
        .map_err(|error| context.error("decode_ledger_splits", error))?
        .into_iter()
        .filter(|row| row.kind == TransactionKind::Split)
        .map(|row| row.trade_date)
        .collect::<Vec<_>>();
    let provider_splits = fundamental_values::active_splits(pool, target.instrument.id)
        .await
        .map_err(|error| context.error("load_provider_splits", error))?
        .into_iter()
        .map(|split| split.date)
        .collect::<Vec<_>>();
    Ok(prepare_lookback(
        today,
        &target.currency,
        closes.last(),
        &closes,
        &ledger_splits,
        &provider_splits,
        mapping.provider.history_is_split_adjusted_at_fetch(),
    ))
}

struct TopUpOutcome {
    history: RefreshItem,
    dividend: Option<RefreshItem>,
}

async fn top_up_one(
    providers: &ProviderSet,
    pool: &SqlitePool,
    mapping: &provider_symbols::ProviderSymbolRow,
    grouped: &BTreeMap<i64, Vec<transactions::TransactionRow>>,
    clock: &Clock,
    context: &HistoryContext<'_>,
    stop_pass: &mut bool,
) -> Result<Option<TopUpOutcome>, MarketDataError> {
    let target = context.target;
    let lease = context.lease;
    let (start, today) = (context.start, context.end);
    if history_topups::attempted_on(pool, mapping.id, today)
        .await
        .map_err(|error| context.error("attempted_on", error))?
    {
        return Ok(None);
    }
    let mut series = load_series(pool, target, mapping, grouped, context).await?;
    let snapshot = NeedSnapshot::new(mapping, &target.currency, &series);
    let fetched_at = clock.now_utc();
    let fetch_date = fetched_at.date_naive();
    // Split-day fetches cannot restate the older closes. Do not consume an
    // attempt when there is no other need that this request could address.
    if snapshot
        .needs
        .iter()
        .all(|need| matches!(need, RemainingNeed::SplitStale { date } if *date >= fetch_date))
    {
        return Ok(None);
    }
    if let Some(old) = history_topups::last_success(pool, mapping.id)
        .await
        .map_err(|error| context.error("last_success", error))?
    {
        let decoded = old
            .remaining_needs
            .as_deref()
            .ok_or_else(|| "missing snapshot".to_owned())
            .and_then(|value| {
                serde_json::from_str::<NeedSnapshot>(value).map_err(|error| error.to_string())
            })
            .and_then(|value| {
                if value.version == 1 {
                    Ok(value)
                } else {
                    Err(format!("unknown snapshot version {}", value.version))
                }
            });
        match decoded {
            Ok(old) if snapshot.covered_by(&old, series.start) => return Ok(None),
            Ok(_) => {}
            Err(error) => crate::engine_warn!("history top-up snapshot decode failure run_id={} instrument_id={} mapping_id={} provider={} symbol={} start={} end={} error={}", lease.run_id, target.instrument.id, mapping.id, mapping.provider, mapping.provider_symbol, start, today, error),
        }
    }
    check_cancelled(lease)?;
    let mapping_id = mapping.id;
    let reserved = market_data_runs::fenced_write(pool, lease.run_id, &lease.owner, |conn| {
        Box::pin(async move { history_topups::begin(conn, mapping_id, today, start, today).await })
    })
    .await
    .map_err(|error| context.error("reserve_attempt", error))?
    .map_err(|_| MarketDataError::LeaseLost)?;
    if !reserved {
        return Ok(None);
    }
    check_cancelled(lease)?;
    // Direct dispatch: a top-up must make exactly one request and never remap a source.
    let result = match providers.price_provider(mapping.provider) {
        Some(client) => {
            client
                .daily_history(&PriceHistoryRequest {
                    symbol: mapping.provider_symbol.clone(),
                    asset_class: mapping.asset_class.clone(),
                    quote_currency: mapping.currency.clone(),
                    start,
                    end: today,
                })
                .await
        }
        None => Err(ProviderError::transport(
            mapping.provider.as_str(),
            "provider_unregistered",
        )),
    }
    .and_then(|history| {
        let latest_close = history.closes.iter().map(|row| row.date).max();
        let rows: Vec<_> = history
            .closes
            .into_iter()
            .filter(|row| row.date >= start && row.date <= today)
            .collect();
        if rows.iter().any(|row| {
            row.provider != mapping.provider || !row.currency.eq_ignore_ascii_case(&target.currency)
        }) {
            Err(ProviderError::provider_error(
                mapping.provider.as_str(),
                "invalid history row provider or currency",
            ))
        } else {
            Ok((
                crate::providers::PriceHistory {
                    closes: rows,
                    dividends: history.dividends,
                },
                latest_close,
            ))
        }
    });
    check_cancelled(lease)?;
    let (rows, dividends, latest_close, status, reason, remaining) = match result {
        Ok((history, latest_close)) => {
            let rows = history.closes;
            for row in &rows {
                series.closes.retain(|close| close.date != row.date);
                if row.date >= series.start {
                    series.closes.push(HistoryClose {
                        date: row.date,
                        close: row.close,
                        currency: row.currency.clone(),
                        provider: ProviderCode::new(mapping.provider.as_str()),
                        fetched_at,
                    });
                }
            }
            series.closes.sort_by_key(|close| close.date);
            // The domain need classifier reads these merged closes; no second policy lives here.
            let mut remaining = NeedSnapshot::new(mapping, &target.currency, &series);
            // A split-day need must be seen as new once UTC advances past it.
            remaining.needs.retain(
                |need| !matches!(need, RemainingNeed::SplitStale { date } if *date >= fetch_date),
            );
            let remaining = serde_json::to_string(&remaining).map_err(|error| {
                context.error(
                    "encode_snapshot",
                    MarketDataError::internal(error.to_string()),
                )
            })?;
            (
                rows,
                history.dividends,
                latest_close,
                RefreshItemStatus::Fetched,
                None,
                Some(remaining),
            )
        }
        Err(error) => {
            *stop_pass = matches!(
                error.reason(),
                ProviderMissingReason::RateLimited | ProviderMissingReason::ProviderUnavailable
            );
            crate::engine_warn!("history top-up failure run_id={} instrument_id={} provider={} symbol={} start={} end={} operation=daily_history reason={} message={}", lease.run_id, target.instrument.id, mapping.provider, mapping.provider_symbol, start, today, error.reason_code(), error.message());
            (
                Vec::new(),
                crate::providers::DividendCapture::NotReported,
                None,
                provider_error_status(&error),
                Some(error.reason_code().to_owned()),
                None,
            )
        }
    };
    let count = rows.len();
    let earliest = rows.iter().map(|row| row.date).min();
    let fetched_at = fetched_at.to_rfc3339();
    let instrument_id = target.instrument.id;
    let provider = mapping.provider;
    let expected_mapping = mapping.clone();
    let dividend = super::dividend_capture::invalid_item(
        &dividends,
        instrument_id,
        provider,
        &mapping.provider_symbol,
    );
    let written = market_data_runs::fenced_write(pool, lease.run_id, &lease.owner, |conn| {
        Box::pin(async move {
            if !history_topups::mapping_is_current(&mut *conn, &expected_mapping).await? {
                history_topups::finish(conn, mapping_id, today, None, None).await?;
                return Ok(false);
            }
            for row in rows {
                prices::upsert(
                    &mut *conn,
                    &prices::NewPrice {
                        instrument_id,
                        provider,
                        provider_symbol: row.provider_symbol,
                        date: row.date,
                        close: row.close,
                        currency: row.currency,
                        fetched_at: fetched_at.clone(),
                    },
                )
                .await?;
            }
            crate::db::dividends::capture(
                conn,
                instrument_id,
                provider,
                start..=today,
                latest_close,
                &dividends,
            )
            .await?;
            history_topups::finish(conn, mapping_id, today, earliest, remaining.as_deref()).await?;
            Ok(true)
        })
    })
    .await;
    let written = match written {
        Ok(written) => written.map_err(|_| MarketDataError::LeaseLost)?,
        Err(error) => {
            // The result transaction rolled back, leaving the reservation. A
            // separate fenced write may still finalize it without publishing rows.
            let cleanup =
                market_data_runs::fenced_write(pool, lease.run_id, &lease.owner, |conn| {
                    Box::pin(async move {
                        history_topups::finish(conn, mapping_id, today, None, None).await
                    })
                })
                .await;
            match cleanup {
                Ok(Ok(())) => {}
                Ok(Err(_)) => {
                    context.error("finalize_failed_attempt", MarketDataError::LeaseLost);
                }
                Err(error) => {
                    context.error("finalize_failed_attempt", error);
                }
            }
            return Err(context.error("write_result", error));
        }
    };
    if !written {
        crate::engine_warn!("history top-up discarded changed mapping run_id={} instrument_id={} provider={} symbol={} start={} end={}", lease.run_id, instrument_id, provider, mapping.provider_symbol, start, today);
        return Ok(Some(TopUpOutcome {
            history: history_item(
                target,
                mapping,
                RefreshItemStatus::Missing,
                Some("mapping_changed".to_owned()),
                0,
            ),
            dividend: None,
        }));
    }
    super::dividend_capture::log_invalid_item(
        dividend.as_ref(),
        lease.run_id,
        provider,
        start,
        today,
    );
    crate::engine_info!("history top-up completed run_id={} instrument_id={} provider={} symbol={} start={} end={} rows={} reason={:?}", lease.run_id, instrument_id, provider, mapping.provider_symbol, start, today, count, reason);
    Ok(Some(TopUpOutcome {
        history: history_item(target, mapping, status, reason, count),
        dividend,
    }))
}

fn check_cancelled(lease: &RefreshLease) -> Result<(), MarketDataError> {
    if lease.cancelled.load(Ordering::Acquire) {
        Err(MarketDataError::LeaseLost)
    } else {
        Ok(())
    }
}

fn history_item(
    target: &RefreshTarget,
    mapping: &provider_symbols::ProviderSymbolRow,
    status: RefreshItemStatus,
    reason: Option<String>,
    rows_written: usize,
) -> RefreshItem {
    RefreshItem {
        kind: RefreshItemKind::History,
        instrument_id: Some(target.instrument.id),
        provider: Some(mapping.provider.to_string()),
        symbol_or_pair: mapping.provider_symbol.clone(),
        status,
        reason,
        rows_written,
    }
}

#[cfg(test)]
mod tests;
