use std::collections::BTreeSet;

use sqlx::sqlite::SqlitePool;

use crate::{db::fx_rates, import::now_iso8601, providers::BASE_FX_PROVIDER};

use super::{
    price_source_refresh::provider_error_status,
    provider_registry::ProviderSet,
    refresh::{RefreshLease, RefreshTarget, RefreshWindow},
    refresh_contract::{MarketDataError, RefreshItem, RefreshItemKind, RefreshItemStatus},
};

const SEK: &str = "SEK";

pub(super) struct FxRefreshOutcome {
    pub(super) attempted_currencies: BTreeSet<String>,
    pub(super) fx_rates_written: usize,
    pub(super) failed_items: usize,
    pub(super) items: Vec<RefreshItem>,
}

pub(super) async fn refresh_fx_rates(
    providers: &ProviderSet,
    pool: &SqlitePool,
    targets: &[RefreshTarget],
    window: &RefreshWindow,
    lease: &RefreshLease,
) -> Result<FxRefreshOutcome, MarketDataError> {
    let mut fx_rates_written = 0usize;
    let mut failed_items = 0usize;
    let mut items = Vec::new();

    let currencies = target_currencies(targets);
    for currency in currencies {
        if currency.eq_ignore_ascii_case(SEK) {
            continue;
        }

        match providers
            .fx_provider()
            .fx_history(&currency, SEK, window.start, window.end)
            .await
        {
            Ok(rows) => {
                let rows_to_write = rows.clone();
                let write = crate::db::market_data_runs::fenced_write(
                    pool,
                    lease.run_id,
                    &lease.owner,
                    |conn| {
                        Box::pin(async move {
                            for row in &rows_to_write {
                                fx_rates::upsert(
                                    &mut *conn,
                                    &fx_rates::NewFxRate {
                                        base: row.base.clone(),
                                        quote: row.quote.clone(),
                                        date: row.date,
                                        rate: row.rate,
                                        provider: BASE_FX_PROVIDER,
                                        fetched_at: now_iso8601(),
                                    },
                                )
                                .await?;
                            }
                            Ok(())
                        })
                    },
                )
                .await?;
                if write.is_err() {
                    return Err(MarketDataError::LeaseLost);
                }
                fx_rates_written += rows.len();
                items.push(RefreshItem {
                    kind: RefreshItemKind::Fx,
                    instrument_id: None,
                    provider: Some(BASE_FX_PROVIDER.to_string()),
                    symbol_or_pair: format!("{currency}/{SEK}"),
                    status: RefreshItemStatus::Fetched,
                    reason: None,
                    rows_written: rows.len(),
                });
            }
            Err(error) => {
                failed_items += 1;
                crate::engine_warn!(
                    "market data refresh fx failure pair={}/{} reason={} message={}",
                    currency,
                    SEK,
                    error.reason_code(),
                    error.message()
                );
                items.push(RefreshItem {
                    kind: RefreshItemKind::Fx,
                    instrument_id: None,
                    provider: Some(BASE_FX_PROVIDER.to_string()),
                    symbol_or_pair: format!("{currency}/{SEK}"),
                    status: provider_error_status(&error),
                    reason: Some(error.reason_code().to_owned()),
                    rows_written: 0,
                });
            }
        }
    }

    Ok(FxRefreshOutcome {
        attempted_currencies: target_currencies(targets),
        fx_rates_written,
        failed_items,
        items,
    })
}

fn target_currencies(targets: &[RefreshTarget]) -> BTreeSet<String> {
    targets
        .iter()
        .map(|target| target.currency.trim().to_ascii_uppercase())
        .filter(|currency| !currency.is_empty())
        .collect()
}

/// Verified ECB reference currencies. Unsupported report currencies remain an
/// explicit FX gap and are never sent to the provider.
pub const ECB_CURRENCIES: &[&str] = &[
    "AUD", "BRL", "CAD", "CHF", "CNY", "CZK", "DKK", "EUR", "GBP", "HKD", "HUF", "IDR", "ILS",
    "INR", "ISK", "JPY", "KRW", "MXN", "MYR", "NOK", "NZD", "PHP", "PLN", "RON", "SEK", "SGD",
    "THB", "TRY", "USD", "ZAR",
];

pub(super) async fn complete_report_fx(
    providers: &ProviderSet,
    pool: &SqlitePool,
    targets: &[RefreshTarget],
    window: &RefreshWindow,
    lease: &RefreshLease,
    attempted: &mut BTreeSet<String>,
) -> Result<Vec<super::fundamentals_refresh::FundamentalsItem>, MarketDataError> {
    use super::fundamentals_refresh::{FundamentalsItem, FundamentalsItemStatus};
    use crate::db::{fundamental_values, fundamentals_sources, market_data_runs};
    let mut needed =
        std::collections::BTreeMap::<String, Vec<(i64, fundamentals_sources::LinkToken)>>::new();
    for target in targets {
        let id = target.instrument.id;
        if let Some(link) = fundamentals_sources::find(pool, id).await? {
            for currency in fundamental_values::report_currencies(pool, id).await? {
                if currency != SEK
                    && ECB_CURRENCIES.contains(&currency.as_str())
                    && !attempted.contains(&currency)
                {
                    needed
                        .entry(currency)
                        .or_default()
                        .push((id, link.token.clone()));
                }
            }
        }
    }
    let mut items = Vec::new();
    for (currency, links) in needed {
        if lease.cancelled.load(std::sync::atomic::Ordering::Acquire) {
            return Err(MarketDataError::LeaseLost);
        }
        attempted.insert(currency.clone());
        let response = providers
            .fx_provider()
            .fx_history(&currency, SEK, window.start, window.end)
            .await;
        let rows = response.and_then(|rows| {
            if rows.is_empty()
                || rows.iter().any(|r| {
                    r.base != currency || r.quote != SEK || r.rate <= rust_decimal::Decimal::ZERO
                })
            {
                Err(crate::providers::ProviderError::new(
                    BASE_FX_PROVIDER.to_string(),
                    crate::providers::ProviderMissingReason::NoDataInRange,
                    "no valid report-currency rates",
                ))
            } else {
                Ok(rows)
            }
        });
        let mut status = FundamentalsItemStatus::Fetched;
        let mut reason = None;
        match rows {
            Ok(rows) => {
                let guards = links.clone();
                let written =
                    market_data_runs::fenced_write(pool, lease.run_id, &lease.owner, |conn| {
                        Box::pin(async move {
                            for (id, token) in guards {
                                if !fundamentals_sources::matches(conn, id, Some(&token)).await? {
                                    return Ok(false);
                                }
                            }
                            for row in rows {
                                fx_rates::upsert(
                                    &mut *conn,
                                    &fx_rates::NewFxRate {
                                        base: row.base,
                                        quote: row.quote,
                                        date: row.date,
                                        rate: row.rate,
                                        provider: BASE_FX_PROVIDER,
                                        fetched_at: now_iso8601(),
                                    },
                                )
                                .await?;
                            }
                            Ok(true)
                        })
                    })
                    .await?;
                match written {
                    Err(_) => return Err(MarketDataError::LeaseLost),
                    Ok(false) => {
                        status = FundamentalsItemStatus::Superseded;
                        reason = Some("superseded".to_owned());
                    }
                    Ok(true) => {}
                }
            }
            Err(error) => {
                status = FundamentalsItemStatus::Failed;
                reason = Some(error.reason_code().to_owned());
                for (id, token) in &links {
                    crate::engine_warn!("fundamentals FX completion failure instrument_id={} symbol={:?} endpoint=https://api.frankfurter.dev/v2/rates pair={}/SEK run_id={} link_generation={} reason={} message={}", id, token.symbol, currency, lease.run_id, token.generation, error.reason_code(), error.message());
                }
            }
        }
        for (id, token) in links {
            items.push(FundamentalsItem {
                instrument_id: id,
                symbol: token.symbol,
                status,
                reason: reason.clone(),
                fx_pair: Some(format!("{currency}/SEK")),
            });
        }
    }
    Ok(items)
}
