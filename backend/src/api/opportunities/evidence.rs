//! Stored-data boundary for opportunity checks. Domain rules own every outcome.
mod response;
pub use response::EvidenceResponse;

use std::collections::BTreeMap;

use chrono::{DateTime, Duration, NaiveDate, Utc};
use rust_decimal::Decimal;

use crate::{
    api::{error::ApiError, fundamentals::loading, valued_holdings::ValuedOpenHolding},
    db::{dividends, prices, RepoError},
    domain::{
        opportunity_evidence::{
            business_check::business_check,
            dividend_check::went_ex_dividend,
            lookback_series::{prepare_lookback, HistoryClose},
            market_reference::{market_reference_with_index, IndexReference},
            parameters::{LOOKBACK_DAYS, NO_REPORT_RULE},
            price_checks::{off_high, stopped_falling, unusual_drop},
            report_timing::{report_coming_soon, report_in_period, ReportTimingInputs},
            room_to_buy::room_to_buy,
            verdict::verdict,
            YesNoUnknown,
        },
        ConvictionTargetOutput, ProviderCode, RecentPriceChange, RecentWindow, TransactionKind,
    },
    market_data::effective_prices::PriceSourceMapping,
    providers::MarketDataProvider,
    state::AppState,
};

pub(super) async fn load(
    state: &AppState,
    today: NaiveDate,
    window: RecentWindow,
    holding: &ValuedOpenHolding,
    target: &ConvictionTargetOutput,
    change: &RecentPriceChange,
    markets: &MarketReferences,
) -> Result<Option<EvidenceResponse>, ApiError> {
    let RecentPriceChange::Available {
        start_date,
        start_close,
        end_date,
        change_percent,
        ..
    } = change
    else {
        return Ok(None);
    };
    if *change_percent >= Decimal::ZERO {
        return Ok(None);
    }
    let fail = |operation, error| log_failure(holding, window, operation, error);
    let latest = holding.latest_price_snapshot().ok_or_else(|| {
        fail(
            "select latest source",
            ApiError::internal("available change without latest close"),
        )
    })?;
    let provider = MarketDataProvider::from_db_str(latest.source.as_str()).ok_or_else(|| {
        fail(
            "decode latest source",
            ApiError::internal("unknown latest price source"),
        )
    })?;
    let stored = prices::list_for_instrument_in_range(
        &state.pool,
        holding.instrument.id,
        provider,
        Some(today - Duration::days(LOOKBACK_DAYS)),
        Some(today),
    )
    .await
    .map_err(|e| fail("load look-back closes", e.into()))?;
    let stored: Vec<_> = stored
        .into_iter()
        .map(decode_close)
        .collect::<Result<_, _>>()
        .map_err(|e| fail("decode look-back closes", e.into()))?;
    let latest_history = stored.iter().find(|c| c.date == latest.date);
    let company = loading::load_inputs(
        &state.pool,
        &holding.instrument,
        today,
        &state.clock,
        Some(latest),
        holding.latest_fx_snapshot(),
        loading::LoadScope::EvidenceWithPriceSources {
            ledger: &holding.ledger,
            price_sources: &markets.price_sources[&holding.instrument.id],
        },
    )
    .await
    .map_err(|e| fail("load company figures and report evidence", e))?;
    let ledger_splits: Vec<_> = holding
        .ledger
        .iter()
        .filter(|t| t.kind == TransactionKind::Split)
        .map(|t| t.trade_date)
        .collect();
    let series = prepare_lookback(
        today,
        &holding.instrument.currency,
        latest_history,
        &stored,
        &ledger_splits,
        &company.provider_splits,
        provider.history_is_split_adjusted_at_fetch(),
    );
    let stored_dividends = dividends::read(
        &state.pool,
        holding.instrument.id,
        provider,
        *start_date,
        *end_date,
    )
    .await
    .map_err(|e| fail("load dividend events and coverage", e.into()))?;
    let dividend = went_ex_dividend(
        *start_date,
        *end_date,
        *start_close,
        &latest.currency,
        stored_dividends.source_reports_dividends,
        &stored_dividends.events,
        &stored_dividends.coverage,
    );
    let adjusted = (dividend.state == YesNoUnknown::Yes)
        .then(|| {
            dividend
                .percent_of_start_close
                .map(|percent| *change_percent + percent)
        })
        .flatten();
    let unusual = unusual_drop(&series, window, *start_date, *change_percent, adjusted);
    let high = off_high(&series);
    let report = report_in_period(
        &ReportTimingInputs {
            today,
            period_start: window.start_date(today),
            latest_period: company.latest,
            source: &company.source,
            last: &company.last_report,
            observations: &company.observations,
            earliest_first_seen: company.earliest_first_seen,
            next: &company.next_report,
            app_next: &company.app_next_report,
        },
        NO_REPORT_RULE,
    );
    let soon = report_coming_soon(today, &company.next_report);
    let business = business_check(
        today,
        &company.figures,
        &company.source,
        &company.next_report,
    );
    let market = market_reference_with_index(
        holding.instrument.id,
        *change_percent,
        window,
        &markets.held_changes,
        (*start_date, *end_date),
        markets.index(holding.instrument.id),
    );
    let stopped = stopped_falling(&series, *start_date, *end_date);
    let room = room_to_buy(target.status);
    let verdict = verdict(&unusual, &high, &report, &business, &soon, room);
    Ok(Some(EvidenceResponse {
        verdict: verdict.into(),
        checks: response::ChecksResponse {
            unusual_drop: unusual.into(),
            off_high: high.into(),
            report_in_period: report.into(),
            report_coming_soon: soon.into(),
            business: business.into(),
            market: market.into(),
            stopped_falling: stopped.into(),
            ex_dividend: dividend.into(),
            room_to_buy: response::Check::room(room, target),
        },
    }))
}

fn decode_close(row: prices::PriceRow) -> Result<HistoryClose, RepoError> {
    Ok(HistoryClose {
        date: row.date_value()?,
        close: row.close_decimal()?,
        provider: ProviderCode::new(row.provider.as_str()),
        fetched_at: DateTime::parse_from_rfc3339(&row.fetched_at)
            .map_err(|e| RepoError::Decode(format!("invalid close fetch time: {e}")))?
            .with_timezone(&Utc),
        currency: row.currency,
    })
}

fn log_failure(
    holding: &ValuedOpenHolding,
    window: RecentWindow,
    operation: &str,
    error: ApiError,
) -> ApiError {
    crate::engine_error!(
        "opportunities evidence failed for instrument {} ({} / {}), window={} operation={}: {:?}",
        holding.instrument.id,
        holding.instrument.exchange,
        holding.instrument.symbol,
        window.as_str(),
        operation,
        error
    );
    error
}

/// Request-scoped inputs: each needed stored index series is read once.
pub(super) struct MarketReferences {
    held_changes: Vec<(i64, Option<Decimal>)>,
    price_sources: BTreeMap<i64, Vec<PriceSourceMapping>>,
    listings: BTreeMap<i64, &'static crate::domain::market_index::MarketIndex>,
    closes: BTreeMap<&'static str, Vec<(NaiveDate, Decimal)>>,
}
impl MarketReferences {
    fn index(&self, instrument_id: i64) -> Option<IndexReference<'_>> {
        let index = self.listings.get(&instrument_id)?;
        Some(IndexReference {
            code: index.code,
            display_name: index.display_name,
            closes: &self.closes[index.code],
        })
    }
}

pub(super) async fn load_market_references<'a>(
    state: &AppState,
    window: RecentWindow,
    rows: impl Iterator<Item = (&'a ValuedOpenHolding, &'a RecentPriceChange)>,
    held_changes: Vec<(i64, Option<Decimal>)>,
) -> Result<MarketReferences, ApiError> {
    use crate::domain::START_CLOSE_MAX_AGE_TRADING_DAYS;
    use crate::{
        db::market_indexes,
        market_data::{effective_prices, listing_market},
    };
    let mut result = MarketReferences {
        held_changes,
        price_sources: Default::default(),
        listings: Default::default(),
        closes: Default::default(),
    };
    let mut needed = BTreeMap::new();
    for (holding, change) in rows {
        let RecentPriceChange::Available {
            start_date,
            end_date,
            change_percent,
            ..
        } = change
        else {
            continue;
        };
        if *change_percent >= Decimal::ZERO {
            continue;
        }
        let sources = effective_prices::enabled_price_sources(&state.pool, holding.instrument.id)
            .await
            .map_err(|e| log_failure(holding, window, "load listing market", e.into()))?;
        if let Some(index) =
            listing_market::from_sources(&sources, &holding.instrument.currency).index()
        {
            result.listings.insert(holding.instrument.id, index);
            // Over-read enough calendar days for the weekday tolerance across weekends.
            // The pure pairing rule applies the exact tolerance to both endpoints.
            let from = *start_date - Duration::days(START_CLOSE_MAX_AGE_TRADING_DAYS * 2 + 1);
            let range = needed.entry(index.code).or_insert((index, from, *end_date));
            range.1 = range.1.min(from);
            range.2 = range.2.max(*end_date);
        }
        result.price_sources.insert(holding.instrument.id, sources);
    }
    for (code, (index, from, to)) in needed {
        let load = async {
            market_indexes::list_in_range(&state.pool, code, MarketDataProvider::Yahoo, index.provider_symbol, from, to).await?
                .into_iter().filter(|row| row.currency.eq_ignore_ascii_case(index.currency))
                .map(|row| Ok((row.date_value()?, row.close_decimal()?))).collect::<Result<Vec<_>, RepoError>>()
        }.await.map_err(|error| {
            crate::engine_error!("opportunities evidence failed: window={} operation=load index closes index={} symbol={} range={}..{}: {}", window.as_str(), code, index.provider_symbol, from, to, error);
            ApiError::from(error)
        })?;
        result.closes.insert(code, load);
    }
    Ok(result)
}
