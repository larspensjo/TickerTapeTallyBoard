use std::collections::BTreeSet;

use sqlx::sqlite::SqlitePool;

use crate::{db::fx_rates, import::now_iso8601, providers::BASE_FX_PROVIDER};

use super::{
    price_source_refresh::provider_error_status,
    provider_registry::ProviderSet,
    refresh::{RefreshTarget, RefreshWindow},
    refresh_contract::{MarketDataError, RefreshItem, RefreshItemKind, RefreshItemStatus},
};

const SEK: &str = "SEK";

pub(super) struct FxRefreshOutcome {
    pub(super) fx_rates_written: usize,
    pub(super) failed_items: usize,
    pub(super) items: Vec<RefreshItem>,
}

pub(super) async fn refresh_fx_rates(
    providers: &ProviderSet,
    pool: &SqlitePool,
    targets: &[RefreshTarget],
    window: &RefreshWindow,
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
                for row in &rows {
                    fx_rates::upsert(
                        pool,
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
