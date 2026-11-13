//! Synthetic market context follows the demo clock and never contacts a provider.
use super::{DemoData, DemoInstrument};
use crate::{
    db::{market_indexes, RepoError},
    domain::market_index::MarketIndex,
    market_data::{effective_prices::PriceSourceMapping, listing_market},
    providers::MarketDataProvider,
};
use chrono::{Datelike, Duration, NaiveDate, Weekday};
use rust_decimal::Decimal;
use sqlx::SqlitePool;
use std::collections::BTreeMap;

fn index(instrument: &DemoInstrument) -> Option<&'static MarketIndex> {
    let sources = [PriceSourceMapping {
        provider: instrument.provider,
        provider_symbol: instrument.provider_symbol.into(),
        asset_class: None,
        currency: Some(instrument.currency.into()),
    }];
    listing_market::from_sources(&sources, instrument.currency).index()
}

// Calendar-day anchors keep the recent swings stable relative to the demo clock.
fn close_at_offset(offset: i64) -> Decimal {
    let anchors = [(0, 980), (7, 1000), (14, 990), (21, 1015), (30, 995)];
    for pair in anchors.windows(2) {
        let [(left_day, left), (right_day, right)] = [pair[0], pair[1]];
        if offset <= right_day {
            return Decimal::from(left)
                + Decimal::from((right - left) * (offset - left_day))
                    / Decimal::from(right_day - left_day);
        }
    }
    Decimal::from(995) - Decimal::from(offset - 30) / Decimal::TEN
}

pub(super) async fn seed(
    pool: &SqlitePool,
    today: NaiveDate,
    data: &DemoData,
) -> Result<(), RepoError> {
    let needed: BTreeMap<_, _> = data
        .instruments
        .iter()
        .filter_map(index)
        .map(|index| (index.code, index))
        .collect();
    let mut tx = pool.begin().await?;
    for index in needed.into_values() {
        for offset in 0..=548 {
            let date = today - Duration::days(offset);
            if matches!(date.weekday(), Weekday::Sat | Weekday::Sun) {
                continue;
            }
            market_indexes::upsert(
                &mut *tx,
                &market_indexes::NewIndexClose {
                    index_code: index.code.into(),
                    provider: MarketDataProvider::Yahoo,
                    date,
                    provider_symbol: index.provider_symbol.into(),
                    close: close_at_offset(offset),
                    currency: index.currency.into(),
                    fetched_at: format!("{today}T12:00:00Z"),
                },
            )
            .await?;
        }
    }
    tx.commit().await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn demo_seeds_every_needed_index_and_only_unmapped_listings_keep_median() {
        for today in ["2026-11-13", "2027-02-12"] {
            let today: NaiveDate = today.parse().unwrap();
            let pool = crate::db::testing::memory_pool().await;
            let data = super::super::dataset(today);
            crate::demo::seed(&pool, today).await.unwrap();
            let median: Vec<_> = data
                .instruments
                .iter()
                .filter(|instrument| index(instrument).is_none())
                .map(|instrument| instrument.symbol)
                .collect();
            assert_eq!(median, ["NORD", "SPRO"]); // Suffix-free SEK listing; Oslo.
            let needed: std::collections::BTreeSet<_> = data
                .instruments
                .iter()
                .filter_map(index)
                .map(|index| index.code)
                .collect();
            let stored: std::collections::BTreeSet<String> =
                sqlx::query_scalar("SELECT DISTINCT index_code FROM market_index_closes")
                    .fetch_all(&pool)
                    .await
                    .unwrap()
                    .into_iter()
                    .collect();
            assert_eq!(
                stored,
                needed.iter().map(|code| (*code).to_owned()).collect()
            );
            for instrument in &data.instruments {
                if let Some(index) = index(instrument) {
                    let rows = market_indexes::list_in_range(
                        &pool,
                        index.code,
                        MarketDataProvider::Yahoo,
                        index.provider_symbol,
                        data.start_date,
                        today,
                    )
                    .await
                    .unwrap();
                    assert!(rows.len() > 370);
                    assert_eq!(rows.last().unwrap().date_value().unwrap(), today);
                    assert!(rows.iter().all(|row| row.currency == index.currency));
                    let recent: Vec<_> = rows
                        .iter()
                        .filter(|row| row.date_value().unwrap() >= today - Duration::days(30))
                        .collect();
                    assert!(recent.windows(2).any(|pair| {
                        pair[1].close_decimal().unwrap() > pair[0].close_decimal().unwrap()
                    }));
                    assert!(recent.windows(2).any(|pair| {
                        pair[1].close_decimal().unwrap() < pair[0].close_decimal().unwrap()
                    }));
                    let last = rows.last().unwrap().close_decimal().unwrap();
                    assert!([7, 14, 30].into_iter().any(|days| {
                        let start = rows
                            .iter()
                            .rev()
                            .find(|row| row.date_value().unwrap() <= today - Duration::days(days))
                            .unwrap();
                        last < start.close_decimal().unwrap()
                    }));
                }
            }
            assert_eq!(
                crate::db::instruments::list(&pool).await.unwrap().len(),
                data.instruments.len()
            );
        }
    }
}
