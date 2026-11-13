//! Refresh only the market context needed by positive-quantity holdings.
use super::{
    effective_prices,
    price_source_refresh::provider_error_status,
    provider_registry::ProviderSet,
    refresh::{position_for_instrument, RefreshLease, RefreshTarget, RefreshWindow},
    refresh_contract::{MarketDataError, RefreshItem, RefreshItemKind, RefreshItemStatus},
};
use crate::{
    clock::Clock,
    db::{market_data_runs, market_indexes, transactions},
    domain::market_index::MarketIndex,
    providers::{MarketDataProvider, PriceHistoryRequest, ProviderError, ProviderMissingReason},
};
use sqlx::SqlitePool;
use std::{collections::BTreeMap, sync::atomic::Ordering};

pub(super) struct IndexRefresh<'a> {
    pub providers: &'a ProviderSet,
    pub pool: &'a SqlitePool,
    pub clock: &'a Clock,
    pub window: &'a RefreshWindow,
    pub lease: &'a RefreshLease,
}

pub(super) async fn run(
    context: IndexRefresh<'_>,
    targets: &[RefreshTarget],
    grouped: &BTreeMap<i64, Vec<transactions::TransactionRow>>,
    items: &mut Vec<RefreshItem>,
) -> Result<(), MarketDataError> {
    let mut needed = BTreeMap::new();
    for target in targets {
        check_cancelled(context.lease)?;
        let result = async {
            if position_for_instrument(grouped, target.instrument.id)?.quantity <= 0 {
                return Ok(None);
            }
            let sources =
                effective_prices::enabled_price_sources(context.pool, target.instrument.id).await?;
            Ok::<_, MarketDataError>(
                super::listing_market::from_sources(&sources, &target.currency).index(),
            )
        }
        .await;
        match result {
            Ok(Some(index)) => {
                needed.insert(index.code, index);
            }
            Ok(None) => {}
            Err(error) => {
                crate::engine_warn!("market index selection failure run_id={} instrument_id={} index=unknown provider=YAHOO symbol=unknown start={} end={} reason={}", context.lease.run_id, target.instrument.id, context.window.start, context.window.end, error);
                items.push(item(
                    None,
                    RefreshItemStatus::Failed,
                    Some("repository_error".into()),
                    0,
                ));
            }
        }
    }
    for index in needed.into_values() {
        check_cancelled(context.lease)?;
        // Also check the durable fence before I/O, including after failed requests.
        match market_data_runs::fenced_write(
            context.pool,
            context.lease.run_id,
            &context.lease.owner,
            |_| Box::pin(async { Ok(()) }),
        )
        .await
        {
            Ok(Ok(())) => {}
            Ok(Err(_)) => return Err(MarketDataError::LeaseLost),
            Err(error) => {
                log_failure(&context, index, "repository_error", &error.to_string());
                items.push(item(
                    Some(index),
                    RefreshItemStatus::Failed,
                    Some("repository_error".into()),
                    0,
                ));
                continue;
            }
        }
        let result = fetch(&context, index).await;
        check_cancelled(context.lease)?;
        match result {
            Ok(rows) => {
                let count = rows.len();
                let result = market_data_runs::fenced_write(
                    context.pool,
                    context.lease.run_id,
                    &context.lease.owner,
                    |conn| {
                        Box::pin(async move {
                            for row in rows {
                                market_indexes::upsert(&mut *conn, &row).await?;
                            }
                            Ok(())
                        })
                    },
                )
                .await;
                match result {
                    Ok(Ok(())) => {
                        crate::engine_info!("market index refresh completed run_id={} index={} provider=YAHOO symbol={} start={} end={} rows={}", context.lease.run_id, index.code, index.provider_symbol, context.window.start, context.window.end, count);
                        items.push(item(Some(index), RefreshItemStatus::Fetched, None, count))
                    }
                    Ok(Err(_)) => return Err(MarketDataError::LeaseLost),
                    Err(error) => {
                        log_failure(&context, index, "repository_error", &error.to_string());
                        items.push(item(
                            Some(index),
                            RefreshItemStatus::Failed,
                            Some("repository_error".into()),
                            0,
                        ));
                    }
                }
            }
            Err(error) => {
                log_failure(&context, index, error.reason_code(), error.message());
                items.push(item(
                    Some(index),
                    provider_error_status(&error),
                    Some(error.reason_code().into()),
                    0,
                ));
                if matches!(
                    error.reason(),
                    ProviderMissingReason::RateLimited | ProviderMissingReason::ProviderUnavailable
                ) {
                    break;
                }
            }
        }
    }
    Ok(())
}

async fn fetch(
    context: &IndexRefresh<'_>,
    index: &MarketIndex,
) -> Result<Vec<market_indexes::NewIndexClose>, ProviderError> {
    let provider = MarketDataProvider::Yahoo;
    let client = context
        .providers
        .price_provider(provider)
        .ok_or_else(|| ProviderError::transport(provider.as_str(), "provider_unregistered"))?;
    let history = client
        .daily_history(&PriceHistoryRequest {
            symbol: index.provider_symbol.into(),
            asset_class: None,
            quote_currency: Some(index.currency.into()),
            start: context.window.start,
            end: context.window.end,
        })
        .await?;
    // No dividend or split capture for an index, even if a provider sends events.
    let rows: Vec<_> = history
        .closes
        .into_iter()
        .filter(|row| row.date >= context.window.start && row.date <= context.window.end)
        .collect();
    if rows.is_empty() {
        return Err(ProviderError::new(
            provider.as_str(),
            ProviderMissingReason::NoDataInRange,
            "no index closes in requested range",
        ));
    }
    if rows.iter().any(|row| {
        row.provider != provider
            || row.provider_symbol != index.provider_symbol
            || !row.currency.eq_ignore_ascii_case(index.currency)
            || row.close <= rust_decimal::Decimal::ZERO
    }) {
        return Err(ProviderError::provider_error(
            provider.as_str(),
            "invalid index close provider, symbol, currency or value",
        ));
    }
    let fetched_at = context.clock.now_utc().to_rfc3339();
    Ok(rows
        .into_iter()
        .map(|row| market_indexes::NewIndexClose {
            index_code: index.code.into(),
            provider,
            date: row.date,
            provider_symbol: index.provider_symbol.into(),
            close: row.close,
            currency: index.currency.into(),
            fetched_at: fetched_at.clone(),
        })
        .collect())
}

fn log_failure(context: &IndexRefresh<'_>, index: &MarketIndex, reason: &str, message: &str) {
    crate::engine_warn!("market index refresh failure run_id={} index={} provider=YAHOO symbol={} start={} end={} reason={} message={}", context.lease.run_id, index.code, index.provider_symbol, context.window.start, context.window.end, reason, message);
}
fn check_cancelled(lease: &RefreshLease) -> Result<(), MarketDataError> {
    if lease.cancelled.load(Ordering::Acquire) {
        Err(MarketDataError::LeaseLost)
    } else {
        Ok(())
    }
}
fn item(
    index: Option<&MarketIndex>,
    status: RefreshItemStatus,
    reason: Option<String>,
    rows_written: usize,
) -> RefreshItem {
    RefreshItem {
        kind: RefreshItemKind::MarketIndex,
        instrument_id: None,
        provider: Some(MarketDataProvider::Yahoo.to_string()),
        symbol_or_pair: index.map_or_else(String::new, |index| index.code.into()),
        status,
        reason,
        rows_written,
    }
}
