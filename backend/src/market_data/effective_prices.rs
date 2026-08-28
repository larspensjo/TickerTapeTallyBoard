use chrono::NaiveDate;
use sqlx::sqlite::SqlitePool;

use crate::{
    db::{prices, provider_symbols, RepoError},
    domain::{
        pick_latest_on_or_before, pick_previous_before, resolve_price_series, PriceCandidate,
        ProviderCode,
    },
    providers::{MarketDataProvider, PRICE_PROVIDER_PRECEDENCE},
};

#[derive(Clone, Debug)]
pub struct PriceSourceMapping {
    pub provider: MarketDataProvider,
    pub provider_symbol: String,
    pub asset_class: Option<String>,
    pub currency: Option<String>,
}

pub async fn enabled_price_sources(
    pool: &SqlitePool,
    instrument_id: i64,
) -> Result<Vec<PriceSourceMapping>, RepoError> {
    let mut sources = Vec::new();
    for provider in PRICE_PROVIDER_PRECEDENCE {
        let mapping =
            provider_symbols::find_by_instrument_provider(pool, instrument_id, *provider).await?;
        if let Some(mapping) = mapping.filter(|mapping| mapping.enabled) {
            sources.push(PriceSourceMapping {
                provider: mapping.provider,
                provider_symbol: mapping.provider_symbol,
                asset_class: mapping.asset_class,
                currency: mapping.currency,
            });
        }
    }
    Ok(sources)
}

pub async fn has_price_coverage(pool: &SqlitePool, instrument_id: i64) -> Result<bool, RepoError> {
    Ok(!enabled_price_sources(pool, instrument_id).await?.is_empty())
}

pub async fn effective_series(
    pool: &SqlitePool,
    instrument_id: i64,
    instrument_currency: &str,
    from: Option<NaiveDate>,
    to: Option<NaiveDate>,
) -> Result<Vec<PriceCandidate>, RepoError> {
    let sources = enabled_price_sources(pool, instrument_id).await?;
    let mut series = Vec::with_capacity(sources.len());
    for source in &sources {
        let rows =
            prices::list_for_instrument_in_range(pool, instrument_id, source.provider, from, to)
                .await?;
        series.push(
            rows.into_iter()
                .map(|row| price_candidate(instrument_id, Some(instrument_currency), row))
                .collect::<Result<Vec<_>, _>>()?,
        );
    }
    let slices: Vec<&[PriceCandidate]> = series.iter().map(Vec::as_slice).collect();
    Ok(resolve_price_series(&slices))
}

pub async fn effective_latest_on_or_before(
    pool: &SqlitePool,
    instrument_id: i64,
    date: NaiveDate,
) -> Result<Option<PriceCandidate>, RepoError> {
    let sources = enabled_price_sources(pool, instrument_id).await?;
    let mut candidates = Vec::with_capacity(sources.len());
    for source in &sources {
        let candidate =
            prices::find_latest_on_or_before(pool, instrument_id, source.provider, date)
                .await?
                .map(|row| price_candidate(instrument_id, None, row))
                .transpose()?;
        candidates.push(candidate);
    }
    Ok(pick_latest_on_or_before(&candidates))
}

pub async fn effective_previous_before(
    pool: &SqlitePool,
    instrument_id: i64,
    before_date: NaiveDate,
) -> Result<Option<PriceCandidate>, RepoError> {
    let sources = enabled_price_sources(pool, instrument_id).await?;
    let mut candidates = Vec::with_capacity(sources.len());
    for source in &sources {
        let candidate =
            prices::find_previous_before(pool, instrument_id, source.provider, before_date)
                .await?
                .map(|row| price_candidate(instrument_id, None, row))
                .transpose()?;
        candidates.push(candidate);
    }
    Ok(pick_previous_before(&candidates))
}

fn price_candidate(
    instrument_id: i64,
    instrument_currency: Option<&str>,
    row: prices::PriceRow,
) -> Result<PriceCandidate, RepoError> {
    let date = row.date_value().map_err(|error| {
        crate::engine_error!(
            "effective price decode failure instrument_id={} row_id={} field=date error={}",
            instrument_id,
            row.id,
            error
        );
        RepoError::Decode(format!(
            "price row {} for instrument {} has invalid date: {error}",
            row.id, instrument_id
        ))
    })?;
    let close = row.close_decimal().map_err(|error| {
        crate::engine_error!(
            "effective price decode failure instrument_id={} row_id={} field=close error={}",
            instrument_id,
            row.id,
            error
        );
        RepoError::Decode(format!(
            "price row {} for instrument {} has invalid close: {error}",
            row.id, instrument_id
        ))
    })?;
    if let Some(instrument_currency) = instrument_currency {
        if !row.currency.eq_ignore_ascii_case(instrument_currency) {
            crate::engine_warn!(
                "price-history currency mismatch instrument_id={} row_id={} row_currency={:?} instrument_currency={:?}",
                instrument_id,
                row.id,
                row.currency,
                instrument_currency
            );
        }
    }
    Ok(PriceCandidate {
        date,
        close,
        currency: row.currency,
        source: ProviderCode::new(row.provider.as_str()),
    })
}

#[cfg(test)]
mod tests {
    use chrono::NaiveDate;
    use rust_decimal_macros::dec;

    use super::*;
    use crate::{
        db::{instruments, prices, provider_symbols, testing},
        providers::MarketDataProvider,
    };

    fn d(day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 6, day).expect("valid date")
    }

    async fn instrument(pool: &SqlitePool) -> i64 {
        instruments::upsert(
            pool,
            &instruments::NewInstrument {
                symbol: "MSFT".to_owned(),
                exchange: "NASDAQ".to_owned(),
                name: "Microsoft".to_owned(),
                kind: "STOCK".to_owned(),
                currency: "USD".to_owned(),
                isin: None,
            },
        )
        .await
        .expect("instrument")
        .0
        .id
    }

    async fn mapping(pool: &SqlitePool, instrument_id: i64, enabled: bool) {
        provider_symbols::upsert(
            pool,
            &provider_symbols::NewProviderSymbol {
                instrument_id,
                provider: MarketDataProvider::Yahoo,
                provider_symbol: "MSFT".to_owned(),
                asset_class: None,
                currency: Some("USD".to_owned()),
                enabled,
                created_at: "2026-06-16T08:00:00Z".to_owned(),
                updated_at: "2026-06-16T08:00:00Z".to_owned(),
            },
        )
        .await
        .expect("mapping");
    }

    async fn price(pool: &SqlitePool, instrument_id: i64) {
        prices::upsert(
            pool,
            &prices::NewPrice {
                instrument_id,
                provider: MarketDataProvider::Yahoo,
                provider_symbol: "MSFT".to_owned(),
                date: d(10),
                close: dec!(100),
                currency: "USD".to_owned(),
                fetched_at: "2026-06-16T08:00:00Z".to_owned(),
            },
        )
        .await
        .expect("price");
    }

    #[tokio::test]
    async fn disabled_or_missing_mapping_excludes_provider_rows() {
        let pool = testing::memory_pool().await;
        let id = instrument(&pool).await;
        price(&pool, id).await;
        assert!(effective_series(&pool, id, "USD", None, None)
            .await
            .expect("series")
            .is_empty());

        mapping(&pool, id, false).await;
        assert!(effective_series(&pool, id, "USD", None, None)
            .await
            .expect("series")
            .is_empty());
    }

    #[tokio::test]
    async fn malformed_price_row_is_a_decode_error() {
        let pool = testing::memory_pool().await;
        let id = instrument(&pool).await;
        mapping(&pool, id, true).await;
        sqlx::query("INSERT INTO prices (instrument_id, provider, provider_symbol, date, close, currency, fetched_at) VALUES (?, ?, ?, ?, ?, ?, ?)")
            .bind(id)
            .bind(MarketDataProvider::Yahoo.as_str())
            .bind("MSFT")
            .bind("not-a-date")
            .bind("100")
            .bind("USD")
            .bind("2026-06-16T08:00:00Z")
            .execute(&pool)
            .await
            .expect("row inserts");

        assert!(matches!(
            effective_series(&pool, id, "USD", None, None).await,
            Err(RepoError::Decode(_))
        ));
    }
}
