//! Independent diagnostics for invalid dividend captures; prices keep their outcome.
use super::refresh_contract::{RefreshItem, RefreshItemKind, RefreshItemStatus};
use crate::providers::{DividendCapture, MarketDataProvider};
use chrono::NaiveDate;

pub(super) fn invalid_item(
    capture: &DividendCapture,
    instrument_id: i64,
    provider: MarketDataProvider,
    symbol: &str,
) -> Option<RefreshItem> {
    let DividendCapture::Invalid { reason } = capture else {
        return None;
    };
    Some(RefreshItem {
        kind: RefreshItemKind::Dividend,
        instrument_id: Some(instrument_id),
        provider: Some(provider.to_string()),
        symbol_or_pair: symbol.to_owned(),
        status: RefreshItemStatus::Failed,
        reason: Some(reason.clone()),
        rows_written: 0,
    })
}

/// Emit only after the capture's fenced transaction has committed.
pub(super) fn log_invalid_item(
    item: Option<&RefreshItem>,
    run_id: i64,
    provider: MarketDataProvider,
    start: NaiveDate,
    end: NaiveDate,
) {
    if let Some(item) = item {
        crate::engine_warn!(
            "dividend capture invalid run_id={} instrument_id={} provider={} symbol={} range={}..{} reason={}",
            run_id,
            item.instrument_id.expect("dividend item has an instrument"),
            provider,
            item.symbol_or_pair,
            start,
            end,
            item.reason.as_deref().expect("invalid capture has a reason")
        );
    }
}
