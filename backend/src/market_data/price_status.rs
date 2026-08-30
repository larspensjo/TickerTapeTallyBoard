use chrono::{NaiveDate, Utc};
use rust_decimal::Decimal;
use sqlx::sqlite::SqlitePool;

use crate::{
    db::{fx_rates, instruments, market_data_runs, provider_symbols, transactions},
    market_data::{
        effective_prices::{self, PriceSourceMapping},
        refresh::{group_transactions, position_for_instrument},
    },
    providers::{BASE_FX_PROVIDER, PRICE_PROVIDER_PRECEDENCE},
};

use super::refresh_contract::{
    refresh_mode_from_trigger, refresh_status_from_db, refresh_trigger_from_db,
};
use super::refresh_contract::{
    InstrumentMarketDataStatus, MarketDataError, PriceSnapshotState, PriceSourceStatus,
    PriceStatusResponse, RefreshRunSummary,
};

const SEK: &str = "SEK";

pub(super) async fn price_status(
    pool: &SqlitePool,
    refreshing: bool,
    active_run: Option<RefreshRunSummary>,
) -> Result<PriceStatusResponse, MarketDataError> {
    let latest_run = if refreshing {
        active_run
    } else {
        latest_run_summary(pool).await?
    };

    let instruments = instruments::list(pool).await?;
    let transactions = transactions::all_for_holdings(pool).await?;
    let grouped = group_transactions(transactions);
    let today = Utc::now().date_naive();

    let mut readiness = Vec::new();
    for instrument in instruments {
        let position = position_for_instrument(&grouped, instrument.id)?;

        let price_sources = all_price_sources(pool, instrument.id).await?;
        let enabled_sources = effective_prices::enabled_price_sources(pool, instrument.id).await?;

        let latest_price =
            latest_price_snapshot(pool, instrument.id, &enabled_sources, today).await?;
        let effective_price_source = latest_price.provider.clone();

        let latest_fx = latest_fx_snapshot(pool, &instrument.currency, today).await?;

        readiness.push(InstrumentMarketDataStatus {
            instrument_id: instrument.id,
            exchange: instrument.exchange,
            symbol: instrument.symbol,
            currency: instrument.currency,
            price_sources,
            effective_price_source,
            open_quantity: position.quantity,
            latest_price,
            latest_fx,
        });
    }

    Ok(PriceStatusResponse {
        refreshing,
        latest_run,
        instruments: readiness,
    })
}

async fn latest_run_summary(
    pool: &SqlitePool,
) -> Result<Option<RefreshRunSummary>, MarketDataError> {
    let row = market_data_runs::latest(pool).await?;
    Ok(row.map(|row| RefreshRunSummary {
        run_id: row.id,
        trigger: refresh_trigger_from_db(&row.trigger),
        mode: refresh_mode_from_trigger(&row.trigger),
        status: refresh_status_from_db(&row.status),
        started_at: row.started_at,
        finished_at: row.finished_at,
        message: row.message,
        prices_written: row.prices_written as usize,
        fx_rates_written: row.fx_rates_written as usize,
        unmapped_instruments: row.unmapped_instruments as usize,
        failed_items: row.failed_items as usize,
    }))
}

/// Every mapping an instrument has, enabled or not, in precedence order.
///
/// `effective_prices::enabled_price_sources` deliberately drops disabled rows
/// because valuation must never read behind them; the status endpoint needs the
/// full list so the UI can say a source exists but is switched off.
async fn all_price_sources(
    pool: &SqlitePool,
    instrument_id: i64,
) -> Result<Vec<PriceSourceStatus>, MarketDataError> {
    let mut sources = Vec::new();
    for provider in PRICE_PROVIDER_PRECEDENCE {
        let mapping =
            provider_symbols::find_by_instrument_provider(pool, instrument_id, *provider).await?;
        if let Some(mapping) = mapping {
            sources.push(PriceSourceStatus {
                provider: mapping.provider.to_string(),
                provider_symbol: mapping.provider_symbol,
                asset_class: mapping.asset_class,
                currency: mapping.currency,
                enabled: mapping.enabled,
            });
        }
    }
    Ok(sources)
}

/// Resolve the latest close across every enabled source, so `/api/prices/status`
/// agrees with what valuation actually used.
///
/// `Unmapped` now means "no enabled source at all", not "the Yahoo mapping is
/// off": a disabled mapping alongside an enabled one reports the resolved price.
async fn latest_price_snapshot(
    pool: &SqlitePool,
    instrument_id: i64,
    sources: &[PriceSourceMapping],
    as_of_date: NaiveDate,
) -> Result<PriceSnapshotState, MarketDataError> {
    if sources.is_empty() {
        return Ok(PriceSnapshotState::unmapped());
    }

    let candidate =
        effective_prices::effective_latest_on_or_before(pool, instrument_id, as_of_date).await?;
    let Some(candidate) = candidate else {
        return Ok(PriceSnapshotState::missing("missing_price"));
    };

    let provider = candidate.source.as_str().to_owned();
    let provider_symbol = sources
        .iter()
        .find(|source| source.provider.as_str() == provider)
        .map(|source| source.provider_symbol.clone())
        .unwrap_or_default();

    Ok(PriceSnapshotState::available(
        candidate.date.format("%Y-%m-%d").to_string(),
        candidate.close.to_string(),
        provider,
        provider_symbol,
    ))
}

async fn latest_fx_snapshot(
    pool: &SqlitePool,
    currency: &str,
    as_of_date: NaiveDate,
) -> Result<PriceSnapshotState, MarketDataError> {
    if currency.eq_ignore_ascii_case(SEK) {
        return Ok(PriceSnapshotState::available(
            as_of_date.format("%Y-%m-%d").to_string(),
            Decimal::ONE.to_string(),
            "identity".to_owned(),
            format!("{SEK}/{SEK}"),
        ));
    }

    let row = fx_rates::find_latest_on_or_before(pool, currency, SEK, BASE_FX_PROVIDER, as_of_date)
        .await?;
    Ok(match row {
        Some(row) => PriceSnapshotState::available(
            row.date,
            row.rate,
            row.provider.to_string(),
            format!("{}/{}", row.base, row.quote),
        ),
        None => PriceSnapshotState::missing("missing_fx"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    use super::super::refresh::tests::{
        buy, empty_search, instrument, map_nasdaq_symbol, map_yahoo_symbol, multi_provider_service,
        set_conviction, silent_fake_price, test_state,
    };
    use crate::{
        db::{self, prices},
        import::now_iso8601,
        market_data::SnapshotStatus,
        providers::{FakeFxRateProvider, FakePriceProvider, FxProvider, MarketDataProvider},
    };
    use rust_decimal_macros::dec;

    #[tokio::test]
    async fn status_readiness_includes_convicted_never_traded_instrument() {
        let price_provider = FakePriceProvider::with_provider(MarketDataProvider::Yahoo);
        let fx_provider = FakeFxRateProvider::with_provider(FxProvider::Frankfurter);

        let (pool, service) = test_state(price_provider, fx_provider).await;
        let watch = instrument(&pool, "WATCH", "NASDAQ", "SEK").await;
        map_yahoo_symbol(&pool, watch, "WATCH", true).await;
        set_conviction(&pool, watch, "MEDIUM").await;

        let status = service.status(&pool).await.expect("status should succeed");
        assert_eq!(status.instruments.len(), 1);
        assert_eq!(status.instruments[0].symbol, "WATCH");
        assert_eq!(status.instruments[0].open_quantity, 0);
        let sources = &status.instruments[0].price_sources;
        assert_eq!(sources.len(), 1);
        assert_eq!(sources[0].provider, "YAHOO");
        assert_eq!(sources[0].provider_symbol, "WATCH");
        assert!(sources[0].enabled);
    }

    #[tokio::test]
    async fn price_status_lists_every_source_and_names_the_effective_one() {
        let service = multi_provider_service(
            silent_fake_price(),
            empty_search(MarketDataProvider::Yahoo),
            FakePriceProvider::with_provider(MarketDataProvider::NasdaqNordic),
            empty_search(MarketDataProvider::NasdaqNordic),
            FakeFxRateProvider::with_provider(FxProvider::Frankfurter),
        );
        let pool = db::memory_pool().await.expect("memory pool");
        let azn = instrument(&pool, "AZN", "STO", "SEK").await;
        buy(&pool, azn, "2026-06-01", 10, "1300", "SEK", None).await;
        map_yahoo_symbol(&pool, azn, "AZN.L", false).await;
        map_nasdaq_symbol(&pool, azn, "TX271", "SHARES", "SEK", true).await;
        prices::upsert(
            &pool,
            &prices::NewPrice {
                instrument_id: azn,
                provider: MarketDataProvider::NasdaqNordic,
                provider_symbol: "TX271".to_owned(),
                date: Utc::now().date_naive(),
                close: dec!(1369.00),
                currency: "SEK".to_owned(),
                fetched_at: now_iso8601(),
            },
        )
        .await
        .expect("nasdaq row should insert");

        let status = service.status(&pool).await.expect("status should succeed");
        let entry = &status.instruments[0];

        assert_eq!(entry.price_sources.len(), 2);
        assert_eq!(entry.price_sources[0].provider, "YAHOO");
        assert!(!entry.price_sources[0].enabled);
        assert_eq!(entry.price_sources[1].provider, "NASDAQ_NORDIC");
        assert_eq!(
            entry.price_sources[1].asset_class.as_deref(),
            Some("SHARES")
        );
        assert!(entry.price_sources[1].enabled);

        assert_eq!(
            entry.effective_price_source.as_deref(),
            Some("NASDAQ_NORDIC")
        );
        assert!(matches!(
            entry.latest_price.status,
            SnapshotStatus::Available
        ));
    }
}
