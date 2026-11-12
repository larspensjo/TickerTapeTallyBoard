use super::*;
use axum::{
    body::{to_bytes, Body},
    http::{Request, StatusCode},
};
use serde_json::Value;
use std::collections::HashSet;
use tower::ServiceExt;

use crate::providers::PRICE_PROVIDER_PRECEDENCE;

#[test]
fn dataset_contains_expected_instrument_shape() {
    let data = fixed_dataset();

    assert_eq!(data.instruments.len(), 9);
    assert_eq!(
        data.instruments
            .iter()
            .map(|instrument| instrument.currency)
            .collect::<HashSet<_>>(),
        HashSet::from(["USD", "SEK", "EUR", "NOK"])
    );

    for index in 0..data.instruments.len() {
        assert!(
            data.transactions
                .iter()
                .filter(|transaction| transaction.instrument_index == index)
                .count()
                >= 2
        );
        assert!(
            data.transactions
                .iter()
                .filter(|transaction| transaction.instrument_index == index
                    && transaction.kind == TransactionKind::Dividend)
                .count()
                >= if data.instruments[index]
                    .scenario
                    .prices
                    .history_days
                    .is_some()
                {
                    1
                } else {
                    2
                }
        );
    }
}

#[test]
fn chart_history_has_modest_daily_moves_and_trade_markers_on_the_price_path() {
    let data = fixed_dataset();
    for (index, instrument) in data.instruments.iter().enumerate() {
        let mut prices: Vec<_> = data
            .prices
            .iter()
            .filter(|p| p.instrument_index == index)
            .collect();
        prices.sort_by_key(|p| p.date);
        for pair in prices.windows(2) {
            // The 2:1 split is the only intentional discontinuity; gaps are not daily moves.
            if pair[1].date - pair[0].date != Duration::days(1)
                || (instrument.scenario.prices.split_stale
                    && pair[1].date == data.end_date - Duration::days(price_paths::SPLIT_AGE_DAYS))
            {
                continue;
            }
            let change = (pair[1].close / pair[0].close - Decimal::ONE).abs();
            assert!(change < dec!(0.03), "{}: {pair:?}", instrument.symbol);
        }
        let historical: Vec<_> = prices
            .iter()
            .filter(|p| p.date < data.end_date - Duration::days(30))
            .map(|p| p.close)
            .collect();
        let low = *historical.iter().min().unwrap();
        let high = *historical.iter().max().unwrap();
        assert!(
            high / low > dec!(1.02),
            "{}: flat history",
            instrument.symbol
        );
    }
    for transaction in data.transactions.iter().filter(|t| t.price.is_some()) {
        let close = data
            .prices
            .iter()
            .find(|p| {
                p.instrument_index == transaction.instrument_index
                    && p.date == transaction.trade_date
            })
            .unwrap()
            .close;
        assert_eq!(transaction.price, Some(close), "{transaction:?}");
    }
}

#[test]
fn dividend_rows_have_dividend_shape() {
    let data = fixed_dataset();

    for transaction in data
        .transactions
        .iter()
        .filter(|transaction| transaction.kind == TransactionKind::Dividend)
    {
        assert!(transaction.dividend_per_share.is_some());
        assert!(transaction.price.is_none());
        assert!(transaction.brokerage.is_none());
    }
}

#[test]
fn non_sek_transactions_carry_fx_rate() {
    let data = fixed_dataset();

    for transaction in &data.transactions {
        let currency = data.instruments[transaction.instrument_index].currency;
        if currency != BASE_CURRENCY {
            assert!(transaction.fx_rate_to_base.is_some());
        }
    }
}

#[test]
fn transaction_dates_are_inside_market_data_window() {
    let data = fixed_dataset();

    for transaction in &data.transactions {
        assert!(transaction.trade_date >= data.start_date);
        assert!(transaction.trade_date <= data.end_date);

        let has_price = data.prices.iter().any(|price| {
            price.instrument_index == transaction.instrument_index
                && price.date == transaction.trade_date
        });
        assert!(has_price);

        let currency = data.instruments[transaction.instrument_index].currency;
        if currency != BASE_CURRENCY {
            let has_fx = data
                .fx_rates
                .iter()
                .any(|fx_rate| fx_rate.base == currency && fx_rate.date == transaction.trade_date);
            assert!(has_fx);
        }
    }
}

#[test]
fn dataset_is_deterministic_for_fixed_date() {
    let today = NaiveDate::from_ymd_opt(2026, 7, 2).expect("date");

    assert_eq!(dataset(today), dataset(today));
}

fn fixed_dataset() -> DemoData {
    dataset(NaiveDate::from_ymd_opt(2026, 7, 2).expect("date"))
}

#[tokio::test]
async fn seed_writes_readable_ledgers_and_market_data() {
    let pool = crate::db::testing::memory_pool().await;
    let today = NaiveDate::from_ymd_opt(2026, 7, 2).expect("date");

    seed_for_date(&pool, today)
        .await
        .expect("seed should succeed");

    let instruments = crate::db::instruments::list(&pool)
        .await
        .expect("instruments should list");
    assert_eq!(instruments.len(), 9);

    for instrument in &instruments {
        let ledger = crate::db::transactions::ledger_for_instrument(&pool, instrument.id)
            .await
            .expect("ledger should load");
        assert!(ledger.len() >= 2);
        crate::domain::derive_position_performance(&ledger).expect("ledger should derive");

        // Resolved across every source, not read off Yahoo: the demo set
        // deliberately contains a Nasdaq-priced instrument.
        let latest_price = crate::market_data::effective_prices::effective_latest_on_or_before(
            &pool,
            instrument.id,
            today,
        )
        .await
        .expect("price lookup should succeed");
        assert!(latest_price.is_some());

        let sources =
            crate::market_data::effective_prices::enabled_price_sources(&pool, instrument.id)
                .await
                .expect("price sources should load");
        assert_eq!(sources.len(), 1);

        // The demo set deliberately contains one Nasdaq-priced holding so
        // the default configuration exercises multi-source resolution.
        let expected_provider = if instrument.symbol == "VARV" {
            PRICE_PROVIDER_PRECEDENCE[1]
        } else {
            PRICE_PROVIDER_PRECEDENCE[0]
        };
        assert_eq!(sources[0].provider, expected_provider);
        assert_eq!(
            latest_price.expect("price").source.as_str(),
            expected_provider.as_str()
        );
    }

    for currency in ["USD", "EUR", "NOK"] {
        let latest_fx = crate::db::fx_rates::find_latest_on_or_before(
            &pool,
            currency,
            BASE_CURRENCY,
            BASE_FX_PROVIDER,
            today,
        )
        .await
        .expect("fx lookup should succeed");
        assert!(latest_fx.is_some());
    }
}

#[tokio::test]
async fn seeded_data_is_available_through_read_endpoints() {
    let pool = crate::db::testing::memory_pool().await;
    let today = NaiveDate::from_ymd_opt(2026, 7, 2).expect("date");
    seed_for_date(&pool, today)
        .await
        .expect("seed should succeed");
    let state = crate::state::AppState::new(
        pool,
        std::sync::Arc::new(crate::market_data::MarketDataService::live()),
    )
    .with_mode(crate::config::Mode::Demo)
    .with_clock(crate::clock::Clock::fixed(today));

    let holdings = get_json(&state, "/api/holdings").await;
    assert_eq!(
        holdings["holdings"]
            .as_array()
            .expect("holdings array")
            .len(),
        9
    );
    assert_no_missing_price_or_fx(&holdings);
    for holding in holdings["holdings"].as_array().expect("holdings array") {
        assert_eq!(holding["base"]["status"], "available");
        assert_eq!(
            holding["valuation"]["market_value_base"]["status"],
            "available"
        );
    }

    let gains = get_json(&state, &format!("/api/gains?end_date={today}")).await;
    assert_eq!(gains["rows"].as_array().expect("gains rows").len(), 9);
    assert_no_missing_price_or_fx(&gains);
    assert_eq!(gains["totals"]["excluded_rows"], 0, "{gains}");
    assert_eq!(gains["summary"]["excluded_rows"], 0, "{gains}");

    let value_history = get_json(&state, "/api/portfolio/value-history").await;
    assert!(!value_history["points"]
        .as_array()
        .expect("value-history points")
        .is_empty());
    assert_no_missing_price_or_fx(&value_history);
    for point in value_history["points"].as_array().unwrap() {
        assert_eq!(point["incomplete"], false, "{point}");
        assert_eq!(point["excluded_count"], 0, "{point}");
    }

    let instrument_id = holdings["holdings"][0]["instrument"]["id"]
        .as_i64()
        .expect("instrument id");
    let prices = get_json(&state, &format!("/api/instruments/{instrument_id}/prices")).await;
    assert!(!prices["points"]
        .as_array()
        .expect("price-history points")
        .is_empty());
    assert_no_missing_price_or_fx(&prices);
}

#[tokio::test]
async fn seeded_holdings_expose_conviction_and_targets() {
    let pool = crate::db::testing::memory_pool().await;
    let today = NaiveDate::from_ymd_opt(2026, 7, 2).expect("date");
    seed_for_date(&pool, today)
        .await
        .expect("seed should succeed");
    let state = crate::state::AppState::new(
        pool,
        std::sync::Arc::new(crate::market_data::MarketDataService::live()),
    )
    .with_mode(crate::config::Mode::Demo)
    .with_clock(crate::clock::Clock::fixed(today));

    let holdings = get_json(&state, "/api/holdings").await;
    let holdings = holdings["holdings"].as_array().expect("holdings array");

    // Representative non-Other convictions are visible.
    let convictions: HashSet<&str> = holdings
        .iter()
        .map(|h| h["instrument"]["conviction"].as_str().expect("conviction"))
        .collect();
    assert!(convictions.contains("High"));
    assert!(convictions.contains("Medium"));
    assert!(convictions.contains("Low"));
    assert!(convictions.contains("Other"));

    // Convicted holdings expose an available, computed target.
    let computed = holdings.iter().filter(|h| {
        matches!(
            h["conviction_target"]["status"].as_str(),
            Some("below" | "on_target" | "above")
        ) && h["conviction_target"]["target_value_base"]["status"] == "available"
    });
    assert!(
        computed.count() >= 1,
        "at least one holding should show a computed target indicator"
    );

    // At least one convicted holding is visibly off target.
    let off_target = holdings.iter().any(|h| {
        matches!(
            h["conviction_target"]["status"].as_str(),
            Some("below" | "above")
        )
    });
    assert!(off_target, "demo data should show a below/above target");
}

pub(super) async fn get_json(state: &crate::state::AppState, uri: &str) -> Value {
    let response = crate::api::router(state.clone())
        .oneshot(
            Request::builder()
                .uri(uri)
                .body(Body::empty())
                .expect("request should build"),
        )
        .await
        .expect("request should complete");
    assert_eq!(response.status(), StatusCode::OK, "{uri}");

    let body = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("body should be readable");
    serde_json::from_slice(&body).expect("body should be JSON")
}

fn assert_no_missing_price_or_fx(value: &Value) {
    assert_no_missing_price_or_fx_at(value, "$");
}

fn assert_no_missing_price_or_fx_at(value: &Value, path: &str) {
    match value {
        Value::String(value) => {
            assert_ne!(value, "missing_price", "{path}");
            assert_ne!(value, "missing_fx", "{path}");
        }
        Value::Array(values) => {
            for (index, value) in values.iter().enumerate() {
                assert_no_missing_price_or_fx_at(value, &format!("{path}[{index}]"));
            }
        }
        Value::Object(values) => {
            for (key, value) in values {
                assert_no_missing_price_or_fx_at(value, &format!("{path}.{key}"));
            }
        }
        Value::Null | Value::Bool(_) | Value::Number(_) => {}
    }
}
