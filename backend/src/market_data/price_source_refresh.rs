use chrono::Duration;
use sqlx::sqlite::SqlitePool;

use crate::{
    db::{prices, provider_symbols},
    import::now_iso8601,
    market_data::{effective_prices::PriceSourceMapping, symbol_seeding},
    providers::{MarketDataProvider, PriceHistoryRequest, ProviderError, ProviderMissingReason},
};

use super::{
    provider_registry::ProviderSet,
    refresh::{RefreshTarget, RefreshWindow},
    refresh_contract::{
        MarketDataError, RefreshItem, RefreshItemKind, RefreshItemStatus, RefreshMode,
    },
    symbol_seeding::ResolvedPriceHistory,
};

/// How far after a requested backfill start the earliest returned row may fall
/// before the run reports `history_clamped`. Nasdaq silently truncates history
/// to roughly ten years and says nothing, so the gap is the only signal.
pub(super) const HISTORY_CLAMP_TOLERANCE_DAYS: i64 = 5;

/// Fetch and store one instrument's prices from one enabled mapping.
///
/// A failure disables or reports only *this* mapping; the instrument's other
/// sources are fetched independently by the caller.
pub(super) async fn refresh_one_source(
    providers: &ProviderSet,
    pool: &SqlitePool,
    target: &RefreshTarget,
    source: &PriceSourceMapping,
    window: &RefreshWindow,
    mode: RefreshMode,
) -> Result<SourceRefreshOutcome, MarketDataError> {
    let instrument = &target.instrument;
    let mapped_symbol = source.provider_symbol.clone();

    if providers.price_provider(source.provider).is_none() {
        crate::engine_warn!(
            "market data refresh has no registered client for a stored mapping instrument_id={} provider={} provider_symbol={}",
            instrument.id,
            source.provider,
            mapped_symbol
        );
        return Ok(SourceRefreshOutcome::failed(RefreshItem {
            kind: RefreshItemKind::Price,
            instrument_id: Some(instrument.id),
            provider: Some(source.provider.to_string()),
            symbol_or_pair: mapped_symbol,
            status: RefreshItemStatus::Unavailable,
            reason: Some("provider_unregistered".to_owned()),
            rows_written: 0,
        }));
    }

    let resolved = match price_history_for_source(providers, instrument, source, window).await {
        Ok(resolved) => resolved,
        Err(error) => {
            crate::engine_warn!(
                "market data refresh price failure instrument_id={} provider={} symbol={} asset_class={:?} reason={} message={}",
                instrument.id,
                source.provider,
                mapped_symbol,
                source.asset_class,
                error.reason_code(),
                error.message()
            );
            return Ok(SourceRefreshOutcome::failed(RefreshItem {
                kind: RefreshItemKind::Price,
                instrument_id: Some(instrument.id),
                provider: Some(source.provider.to_string()),
                symbol_or_pair: mapped_symbol,
                status: provider_error_status(&error),
                reason: Some(error.reason_code().to_owned()),
                rows_written: 0,
            }));
        }
    };

    let provider_symbol = resolved.provider_symbol;
    let rows = resolved.rows;

    if !provider_symbol.eq_ignore_ascii_case(&mapped_symbol) {
        // The recovery path only accepts candidates whose rows quote in the
        // instrument's own currency, so that is what the mapping records.
        persist_mapping(
            pool,
            instrument,
            source,
            &provider_symbol,
            Some(instrument.currency.clone()),
            true,
        )
        .await?;
        crate::engine_info!(
            "market data recovered changed yahoo symbol instrument_id={} isin={:?} old_symbol={} new_symbol={}",
            instrument.id,
            instrument.isin,
            mapped_symbol,
            provider_symbol
        );
    }

    if let Some(row) = rows.iter().find(|row| {
        !row.currency
            .trim()
            .eq_ignore_ascii_case(instrument.currency.trim())
    }) {
        // Keep the currency the mapping claimed: for Nasdaq it is what
        // stamped the rows, and it is the value that needs correcting.
        persist_mapping(
            pool,
            instrument,
            source,
            &provider_symbol,
            source
                .currency
                .clone()
                .or_else(|| Some(instrument.currency.clone())),
            false,
        )
        .await?;
        crate::engine_warn!(
            "market data refresh price currency mismatch instrument_id={} provider={} symbol={} expected_currency={} actual_currency={}; disabled mapping",
            instrument.id,
            source.provider,
            provider_symbol,
            instrument.currency,
            row.currency
        );
        return Ok(SourceRefreshOutcome::failed(RefreshItem {
            kind: RefreshItemKind::Price,
            instrument_id: Some(instrument.id),
            provider: Some(source.provider.to_string()),
            symbol_or_pair: provider_symbol,
            status: RefreshItemStatus::Failed,
            reason: Some("currency_mismatch".to_owned()),
            rows_written: 0,
        }));
    }

    for row in &rows {
        prices::upsert(
            pool,
            &prices::NewPrice {
                instrument_id: instrument.id,
                provider: source.provider,
                provider_symbol: row.provider_symbol.clone(),
                date: row.date,
                close: row.close,
                currency: row.currency.clone(),
                fetched_at: now_iso8601(),
            },
        )
        .await?;
    }

    let reason = clamp_reason(instrument, source, &provider_symbol, &rows, window, mode);

    Ok(SourceRefreshOutcome::succeeded(RefreshItem {
        kind: RefreshItemKind::Price,
        instrument_id: Some(instrument.id),
        provider: Some(source.provider.to_string()),
        symbol_or_pair: provider_symbol,
        status: RefreshItemStatus::Fetched,
        reason,
        rows_written: rows.len(),
    }))
}

/// Rewrite one mapping's enabled flag (and, after a symbol recovery, its
/// identifier) while preserving the asset class and currency that make a
/// Nasdaq mapping fetchable at all.
pub(super) async fn persist_mapping(
    pool: &SqlitePool,
    instrument: &crate::db::instruments::InstrumentRow,
    source: &PriceSourceMapping,
    provider_symbol: &str,
    currency: Option<String>,
    enabled: bool,
) -> Result<(), MarketDataError> {
    let now = now_iso8601();
    provider_symbols::upsert(
        pool,
        &provider_symbols::NewProviderSymbol {
            instrument_id: instrument.id,
            provider: source.provider,
            provider_symbol: provider_symbol.to_owned(),
            asset_class: source.asset_class.clone(),
            currency,
            enabled,
            created_at: now.clone(),
            updated_at: now,
        },
    )
    .await?;
    Ok(())
}

/// Yahoo mappings go through the changed-symbol recovery path; every other
/// provider is a straight fetch against the mapping as stored.
pub(super) async fn price_history_for_source(
    providers: &ProviderSet,
    instrument: &crate::db::instruments::InstrumentRow,
    source: &PriceSourceMapping,
    window: &RefreshWindow,
) -> Result<ResolvedPriceHistory, ProviderError> {
    if source.provider == MarketDataProvider::Yahoo {
        return symbol_seeding::price_history_with_symbol_recovery(
            instrument,
            &source.provider_symbol,
            window,
            providers.price_provider(MarketDataProvider::Yahoo),
            providers.symbol_search_provider(MarketDataProvider::Yahoo),
        )
        .await;
    }

    let client = providers.price_provider(source.provider).ok_or_else(|| {
        ProviderError::provider_error(
            source.provider.as_str(),
            format!("no registered client for provider {}", source.provider),
        )
    })?;
    let rows = client
        .daily_history(&PriceHistoryRequest {
            symbol: source.provider_symbol.clone(),
            asset_class: source.asset_class.clone(),
            quote_currency: source.currency.clone(),
            start: window.start,
            end: window.end,
        })
        .await?;
    Ok(ResolvedPriceHistory::new(&source.provider_symbol, rows))
}

/// The result of fetching one instrument from one of its sources.
pub(super) struct SourceRefreshOutcome {
    pub(super) item: RefreshItem,
    pub(super) failed: bool,
}

impl SourceRefreshOutcome {
    fn succeeded(item: RefreshItem) -> Self {
        Self {
            item,
            failed: false,
        }
    }

    fn failed(item: RefreshItem) -> Self {
        Self { item, failed: true }
    }
}

pub(super) fn provider_error_status(error: &ProviderError) -> RefreshItemStatus {
    match error.reason() {
        ProviderMissingReason::SymbolUnmapped => RefreshItemStatus::Unmapped,
        ProviderMissingReason::NoDataInRange => RefreshItemStatus::Missing,
        ProviderMissingReason::NotListed | ProviderMissingReason::MarketClosed => {
            RefreshItemStatus::Missing
        }
        ProviderMissingReason::RateLimited | ProviderMissingReason::ProviderError => {
            RefreshItemStatus::Failed
        }
        ProviderMissingReason::ProviderUnavailable => RefreshItemStatus::Unavailable,
    }
}

/// Detect a provider silently truncating requested history.
///
/// Nasdaq clamps to roughly ten years with no error and no warning, and a
/// backfill cannot otherwise tell "no data" from "older than the provider keeps".
/// This is a heuristic with benign false positives — a recently-listed
/// instrument or a long holiday stretch trips it with no clamp involved — so it
/// is warning-only and the reason must not be read as proof of a clamp.
pub(super) fn clamp_reason(
    instrument: &crate::db::instruments::InstrumentRow,
    source: &PriceSourceMapping,
    provider_symbol: &str,
    rows: &[crate::providers::DailyClose],
    window: &RefreshWindow,
    mode: RefreshMode,
) -> Option<String> {
    if mode != RefreshMode::Backfill || source.provider != MarketDataProvider::NasdaqNordic {
        return None;
    }
    let earliest = rows.iter().map(|row| row.date).min()?;
    if earliest - window.start <= Duration::days(HISTORY_CLAMP_TOLERANCE_DAYS) {
        return None;
    }

    crate::engine_warn!(
        "market data history clamped instrument_id={} provider={} orderbook_id={} requested_start={} actual_start={}",
        instrument.id,
        source.provider,
        provider_symbol,
        window.start,
        earliest
    );
    Some("history_clamped".to_owned())
}
