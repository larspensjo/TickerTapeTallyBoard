use std::collections::BTreeMap;

use sqlx::sqlite::SqlitePool;

use crate::{
    db::{instruments, transactions},
    domain,
    market_data::effective_prices,
    providers::MarketDataProvider,
};

use super::{
    fx_refresh, price_source_refresh,
    provider_registry::{self, ProviderSet},
    refresh,
    refresh_contract::{
        MarketDataError, RefreshItem, RefreshItemKind, RefreshItemStatus, RefreshMode,
        RefreshPricesRequest, RefreshRunStatus,
    },
    symbol_seeding,
};

pub(super) struct RefreshOutcome {
    pub(super) status: RefreshRunStatus,
    pub(super) message: Option<String>,
    pub(super) prices_written: usize,
    pub(super) fx_rates_written: usize,
    pub(super) unmapped_instruments: usize,
    pub(super) failed_items: usize,
    pub(super) items: Vec<RefreshItem>,
}

pub(super) struct RefreshTarget {
    pub(super) instrument: crate::db::instruments::InstrumentRow,
    pub(super) currency: String,
}

pub(super) async fn execute_refresh(
    providers: &ProviderSet,
    pool: &SqlitePool,
    request: &RefreshPricesRequest,
) -> Result<RefreshOutcome, MarketDataError> {
    let target_window = refresh::refresh_window(request, pool).await?;
    let transactions = transactions::all_for_holdings(pool).await?;
    let grouped = group_transactions(transactions);
    let instruments = instruments::list(pool).await?;
    let ambiguous = symbol_seeding::seed_provider_symbols(
        pool,
        &instruments,
        providers.symbol_search_providers(),
    )
    .await?;

    let mut targets = Vec::new();
    for instrument in instruments {
        let has_history = grouped.contains_key(&instrument.id);
        let position = position_for_instrument(&grouped, instrument.id)?;
        let should_skip = match request.mode {
            RefreshMode::Latest => {
                let conviction = match domain::ConvictionLevel::from_db_str(&instrument.conviction)
                {
                    Some(conviction) => conviction,
                    None => {
                        return Err(MarketDataError::internal(format!(
                            "stored unknown conviction {:?} for instrument {}",
                            instrument.conviction, instrument.id
                        )));
                    }
                };
                position.quantity == 0
                    && !matches!(
                        conviction,
                        domain::ConvictionLevel::Low
                            | domain::ConvictionLevel::Medium
                            | domain::ConvictionLevel::High
                    )
            }
            RefreshMode::Backfill => !has_history,
        };
        if should_skip {
            continue;
        }

        let currency = instrument.currency.clone();
        targets.push(RefreshTarget {
            instrument,
            currency,
        });
    }

    let mut prices_written = 0usize;
    let mut fx_rates_written = 0usize;
    let mut unmapped_instruments = 0usize;
    let mut failed_items = 0usize;
    let mut items = Vec::new();
    let mut rows_by_provider: Vec<(MarketDataProvider, usize)> = Vec::new();

    for target in &targets {
        let sources = effective_prices::enabled_price_sources(pool, target.instrument.id).await?;

        if sources.is_empty() {
            unmapped_instruments += 1;
            items.push(unsourced_item(target, &ambiguous));
            continue;
        }

        for source in &sources {
            let outcome = price_source_refresh::refresh_one_source(
                providers,
                pool,
                target,
                source,
                &target_window,
                request.mode,
            )
            .await?;
            if outcome.failed {
                failed_items += 1;
            }
            prices_written += outcome.item.rows_written;
            if outcome.item.rows_written > 0 {
                provider_registry::add_provider_rows(
                    &mut rows_by_provider,
                    source.provider,
                    outcome.item.rows_written,
                );
            }
            items.push(outcome.item);
        }
    }

    let fx_outcome =
        fx_refresh::refresh_fx_rates(providers, pool, &targets, &target_window).await?;
    fx_rates_written += fx_outcome.fx_rates_written;
    failed_items += fx_outcome.failed_items;
    items.extend(fx_outcome.items);

    let status = if failed_items == 0 && unmapped_instruments == 0 {
        RefreshRunStatus::Succeeded
    } else if prices_written == 0 && fx_rates_written == 0 {
        RefreshRunStatus::Failed
    } else {
        RefreshRunStatus::Partial
    };

    let by_provider = provider_registry::describe_provider_split(&rows_by_provider);
    crate::engine_info!(
        "market data refresh provider split {by_provider} unmapped={unmapped_instruments} failed={failed_items}"
    );
    let message = Some(format!(
        "prices_written={prices_written} fx_rates_written={fx_rates_written} unmapped={unmapped_instruments} failed={failed_items} by_provider=[{by_provider}]"
    ));

    Ok(RefreshOutcome {
        status,
        message,
        prices_written,
        fx_rates_written,
        unmapped_instruments,
        failed_items,
        items,
    })
}

pub(super) fn group_transactions(
    rows: Vec<crate::db::transactions::TransactionRow>,
) -> BTreeMap<i64, Vec<crate::db::transactions::TransactionRow>> {
    let mut grouped: BTreeMap<i64, Vec<crate::db::transactions::TransactionRow>> = BTreeMap::new();
    for row in rows {
        grouped.entry(row.instrument_id).or_default().push(row);
    }
    grouped
}

pub(super) fn position_for_instrument(
    grouped: &BTreeMap<i64, Vec<crate::db::transactions::TransactionRow>>,
    instrument_id: i64,
) -> Result<domain::Position, MarketDataError> {
    let ledger = match grouped.get(&instrument_id) {
        Some(rows) => rows
            .iter()
            .map(|row| row.to_ledger())
            .collect::<Result<Vec<_>, _>>()?,
        None => Vec::new(),
    };
    domain::derive_position(&ledger).map_err(|error| {
        MarketDataError::internal(format!(
            "inconsistent stored ledger for instrument {}: {error:?}",
            instrument_id
        ))
    })
}

/// Report an instrument that ended the run with no usable price source.
///
/// An ambiguity is a distinct state from "no provider carries this": it
/// means a hand mapping would fix it, so it is named rather than folded
/// into `Unmapped`. Both count as unmapped, because action is needed.
fn unsourced_item(target: &RefreshTarget, ambiguous: &BTreeMap<i64, String>) -> RefreshItem {
    match ambiguous.get(&target.instrument.id) {
        Some(candidates) => RefreshItem {
            kind: RefreshItemKind::Price,
            instrument_id: Some(target.instrument.id),
            provider: Some(MarketDataProvider::NasdaqNordic.to_string()),
            symbol_or_pair: target.instrument.symbol.clone(),
            status: RefreshItemStatus::Ambiguous,
            reason: Some(format!("ambiguous_match: {candidates}")),
            rows_written: 0,
        },
        None => RefreshItem {
            kind: RefreshItemKind::Price,
            instrument_id: Some(target.instrument.id),
            provider: None,
            symbol_or_pair: target.instrument.symbol.clone(),
            status: RefreshItemStatus::Unmapped,
            reason: Some("symbol_unmapped".to_owned()),
            rows_written: 0,
        },
    }
}
