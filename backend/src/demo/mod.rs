use chrono::{Datelike, Duration, NaiveDate};
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use sqlx::sqlite::SqlitePool;

use crate::db::{fx_rates, instruments, prices, provider_symbols, transactions};
use crate::domain::TransactionKind;

pub const BASE_CURRENCY: &str = "SEK";
pub const PRICE_PROVIDER: &str = "YAHOO";
pub const FX_PROVIDER: &str = "FRANKFURTER";

#[derive(Clone, Debug, PartialEq)]
pub struct DemoData {
    pub start_date: NaiveDate,
    pub end_date: NaiveDate,
    pub instruments: Vec<DemoInstrument>,
    pub transactions: Vec<DemoTransaction>,
    pub prices: Vec<DemoPrice>,
    pub fx_rates: Vec<DemoFxRate>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DemoInstrument {
    pub symbol: &'static str,
    pub exchange: &'static str,
    pub name: &'static str,
    pub kind: &'static str,
    pub currency: &'static str,
    pub isin: Option<&'static str>,
    pub provider_symbol: &'static str,
}

#[derive(Clone, Debug, PartialEq)]
pub struct DemoTransaction {
    pub instrument_index: usize,
    pub kind: TransactionKind,
    pub trade_date: NaiveDate,
    pub quantity: i64,
    pub price: Option<Decimal>,
    pub dividend_per_share: Option<Decimal>,
    pub currency: Option<&'static str>,
    pub fx_rate_to_base: Option<Decimal>,
    pub brokerage: Option<Decimal>,
    pub note: Option<&'static str>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct DemoPrice {
    pub instrument_index: usize,
    pub provider: &'static str,
    pub provider_symbol: &'static str,
    pub date: NaiveDate,
    pub close: Decimal,
    pub currency: &'static str,
}

#[derive(Clone, Debug, PartialEq)]
pub struct DemoFxRate {
    pub base: &'static str,
    pub quote: &'static str,
    pub date: NaiveDate,
    pub rate: Decimal,
    pub provider: &'static str,
}

pub fn dataset(today: NaiveDate) -> DemoData {
    let end_date = today;
    let start_date = today - Duration::days(548);
    let instruments = instruments();
    let transactions = transactions(start_date, &instruments);
    let prices = prices(start_date, end_date, &instruments);
    let fx_rates = fx_rates(start_date, end_date);

    DemoData {
        start_date,
        end_date,
        instruments,
        transactions,
        prices,
        fx_rates,
    }
}

pub async fn seed(pool: &SqlitePool) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    seed_for_date(pool, chrono::Local::now().date_naive()).await
}

async fn seed_for_date(
    pool: &SqlitePool,
    today: NaiveDate,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let data = dataset(today);
    let fetched_at = crate::import::now_iso8601();
    let mut instrument_ids = Vec::with_capacity(data.instruments.len());

    for instrument in &data.instruments {
        let (row, _) = instruments::upsert(
            pool,
            &instruments::NewInstrument {
                symbol: instrument.symbol.to_owned(),
                exchange: instrument.exchange.to_owned(),
                name: instrument.name.to_owned(),
                kind: instrument.kind.to_owned(),
                currency: instrument.currency.to_owned(),
                isin: instrument.isin.map(str::to_owned),
            },
        )
        .await?;
        instrument_ids.push(row.id);

        provider_symbols::upsert(
            pool,
            &provider_symbols::NewProviderSymbol {
                instrument_id: row.id,
                provider: PRICE_PROVIDER.to_owned(),
                provider_symbol: instrument.provider_symbol.to_owned(),
                currency: Some(instrument.currency.to_owned()),
                enabled: true,
                created_at: fetched_at.clone(),
                updated_at: fetched_at.clone(),
            },
        )
        .await?;
    }

    for transaction in &data.transactions {
        transactions::insert(
            pool,
            &transactions::NewTransaction {
                instrument_id: instrument_ids[transaction.instrument_index],
                kind: transaction.kind,
                trade_date: transaction.trade_date,
                quantity: transaction.quantity,
                price: transaction.price,
                dividend_per_share: transaction.dividend_per_share,
                currency: transaction.currency.map(str::to_owned),
                fx_rate_to_base: transaction.fx_rate_to_base,
                brokerage: transaction.brokerage,
                note: transaction.note.map(str::to_owned),
            },
        )
        .await?;
    }

    for price in &data.prices {
        prices::upsert(
            pool,
            &prices::NewPrice {
                instrument_id: instrument_ids[price.instrument_index],
                provider: price.provider.to_owned(),
                provider_symbol: price.provider_symbol.to_owned(),
                date: price.date,
                close: price.close,
                currency: price.currency.to_owned(),
                fetched_at: fetched_at.clone(),
            },
        )
        .await?;
    }

    for fx_rate in &data.fx_rates {
        fx_rates::upsert(
            pool,
            &fx_rates::NewFxRate {
                base: fx_rate.base.to_owned(),
                quote: fx_rate.quote.to_owned(),
                date: fx_rate.date,
                rate: fx_rate.rate,
                provider: fx_rate.provider.to_owned(),
                fetched_at: fetched_at.clone(),
            },
        )
        .await?;
    }

    for instrument_id in instrument_ids {
        let ledger = transactions::ledger_for_instrument(pool, instrument_id).await?;
        crate::domain::derive_position_performance(&ledger).map_err(|error| {
            format!("demo ledger for instrument {instrument_id} failed validation: {error:?}")
        })?;
    }

    Ok(())
}

fn instruments() -> Vec<DemoInstrument> {
    vec![
        DemoInstrument {
            symbol: "NOVA",
            exchange: "NASDAQ",
            name: "Nova Systems",
            kind: "STOCK",
            currency: "USD",
            isin: None,
            provider_symbol: "NOVA",
        },
        DemoInstrument {
            symbol: "HARV",
            exchange: "NYSE",
            name: "Harbor Utilities",
            kind: "STOCK",
            currency: "USD",
            isin: None,
            provider_symbol: "HARV",
        },
        DemoInstrument {
            symbol: "NORD",
            exchange: "STO",
            name: "Nordic Automation",
            kind: "STOCK",
            currency: "SEK",
            isin: None,
            provider_symbol: "NORD.ST",
        },
        DemoInstrument {
            symbol: "SKOG",
            exchange: "STO",
            name: "Skog & Marin",
            kind: "STOCK",
            currency: "SEK",
            isin: None,
            provider_symbol: "SKOG.ST",
        },
        DemoInstrument {
            symbol: "ALBA",
            exchange: "XETRA",
            name: "Alba Renewables",
            kind: "STOCK",
            currency: "EUR",
            isin: None,
            provider_symbol: "ALBA.DE",
        },
        DemoInstrument {
            symbol: "GLBL",
            exchange: "NYSEARCA",
            name: "Global Core ETF",
            kind: "ETF",
            currency: "USD",
            isin: None,
            provider_symbol: "GLBL",
        },
    ]
}

fn transactions(start_date: NaiveDate, instruments: &[DemoInstrument]) -> Vec<DemoTransaction> {
    let specs: [Vec<TransactionSpec>; 6] = [
        vec![
            buy(20, 24, dec!(94.20)),
            dividend(118, dec!(0.42)),
            buy(168, 12, dec!(102.75)),
            dividend(214, dec!(0.45)),
            dividend(304, dec!(0.47)),
            sell(356, 8, dec!(118.10)),
            dividend(486, dec!(0.50)),
        ],
        vec![
            buy(45, 32, dec!(38.40)),
            dividend(126, dec!(0.31)),
            buy(231, 18, dec!(41.25)),
            dividend(273, dec!(0.33)),
            dividend(365, dec!(0.35)),
            dividend(512, dec!(0.37)),
        ],
        vec![
            buy(16, 18, dec!(142.00)),
            dividend(151, dec!(2.10)),
            buy(238, 14, dec!(151.50)),
            dividend(322, dec!(2.25)),
            buy(402, 10, dec!(158.40)),
            dividend(525, dec!(2.40)),
        ],
        vec![
            buy(64, 40, dec!(76.50)),
            dividend(184, dec!(1.15)),
            buy(276, 18, dec!(82.80)),
            sell(431, 16, dec!(89.25)),
            dividend(519, dec!(1.22)),
        ],
        vec![
            buy(34, 22, dec!(58.40)),
            dividend(142, dec!(0.72)),
            buy(312, 12, dec!(63.10)),
            dividend(394, dec!(0.76)),
            dividend(536, dec!(0.80)),
        ],
        vec![
            buy(82, 36, dec!(49.20)),
            dividend(246, dec!(0.18)),
            buy(342, 20, dec!(53.65)),
            dividend(526, dec!(0.20)),
        ],
    ];

    specs
        .into_iter()
        .enumerate()
        .flat_map(|(instrument_index, specs)| {
            let instrument = &instruments[instrument_index];
            specs.into_iter().map(move |spec| {
                let trade_date = start_date + Duration::days(spec.day_offset);
                let fx_rate_to_base = Some(fx_rate_for_currency(instrument.currency, trade_date));

                DemoTransaction {
                    instrument_index,
                    kind: spec.kind,
                    trade_date,
                    quantity: spec.quantity,
                    price: spec.price,
                    dividend_per_share: spec.dividend_per_share,
                    currency: Some(instrument.currency),
                    fx_rate_to_base,
                    brokerage: (spec.kind != TransactionKind::Dividend).then_some(dec!(9.00)),
                    note: Some("Demo seed"),
                }
            })
        })
        .collect()
}

fn prices(
    start_date: NaiveDate,
    end_date: NaiveDate,
    instruments: &[DemoInstrument],
) -> Vec<DemoPrice> {
    instruments
        .iter()
        .enumerate()
        .flat_map(|(instrument_index, instrument)| {
            let seed = 17 + instrument_index as i64 * 23;
            let mut date = start_date;
            let mut day = 0_i64;
            let base = match instrument.currency {
                "SEK" => dec!(78.00) + Decimal::from(instrument_index as i64 * 28),
                "EUR" => dec!(56.00),
                _ => dec!(40.00) + Decimal::from(instrument_index as i64 * 13),
            };
            std::iter::from_fn(move || {
                if date > end_date {
                    return None;
                }
                let wave = Decimal::from(((day + seed) % 29) - 14) / dec!(100);
                let drift = Decimal::from(day) / dec!(180);
                let close = (base + drift + wave).round_dp(2).max(dec!(1.00));
                let price = DemoPrice {
                    instrument_index,
                    provider: PRICE_PROVIDER,
                    provider_symbol: instrument.provider_symbol,
                    date,
                    close,
                    currency: instrument.currency,
                };
                date += Duration::days(1);
                day += 1;
                Some(price)
            })
        })
        .collect()
}

fn fx_rates(start_date: NaiveDate, end_date: NaiveDate) -> Vec<DemoFxRate> {
    ["USD", "EUR"]
        .into_iter()
        .flat_map(|currency| {
            let mut date = start_date;
            std::iter::from_fn(move || {
                if date > end_date {
                    return None;
                }
                let rate = fx_rate_for_currency(currency, date);
                let fx_rate = DemoFxRate {
                    base: currency,
                    quote: BASE_CURRENCY,
                    date,
                    rate,
                    provider: FX_PROVIDER,
                };
                date += Duration::days(1);
                Some(fx_rate)
            })
        })
        .collect()
}

fn fx_rate_for_currency(currency: &str, date: NaiveDate) -> Decimal {
    let ordinal = i64::from(date.num_days_from_ce());
    match currency {
        "USD" => (dec!(10.15) + Decimal::from(ordinal % 17) / dec!(100)).round_dp(4),
        "EUR" => (dec!(11.05) + Decimal::from(ordinal % 19) / dec!(100)).round_dp(4),
        BASE_CURRENCY => dec!(1),
        _ => dec!(1),
    }
}

#[derive(Clone, Debug)]
struct TransactionSpec {
    kind: TransactionKind,
    day_offset: i64,
    quantity: i64,
    price: Option<Decimal>,
    dividend_per_share: Option<Decimal>,
}

fn buy(day_offset: i64, quantity: i64, price: Decimal) -> TransactionSpec {
    TransactionSpec {
        kind: TransactionKind::Buy,
        day_offset,
        quantity,
        price: Some(price),
        dividend_per_share: None,
    }
}

fn sell(day_offset: i64, quantity: i64, price: Decimal) -> TransactionSpec {
    TransactionSpec {
        kind: TransactionKind::Sell,
        day_offset,
        quantity: -quantity,
        price: Some(price),
        dividend_per_share: None,
    }
}

fn dividend(day_offset: i64, dividend_per_share: Decimal) -> TransactionSpec {
    TransactionSpec {
        kind: TransactionKind::Dividend,
        day_offset,
        quantity: 0,
        price: None,
        dividend_per_share: Some(dividend_per_share),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        body::{to_bytes, Body},
        http::{Request, StatusCode},
    };
    use serde_json::Value;
    use std::collections::HashSet;
    use tower::ServiceExt;

    #[test]
    fn dataset_contains_expected_instrument_shape() {
        let data = fixed_dataset();

        assert_eq!(data.instruments.len(), 6);
        assert_eq!(
            data.instruments
                .iter()
                .map(|instrument| instrument.currency)
                .collect::<HashSet<_>>(),
            HashSet::from(["USD", "SEK", "EUR"])
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
                    >= 2
            );
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
                let has_fx = data.fx_rates.iter().any(|fx_rate| {
                    fx_rate.base == currency && fx_rate.date == transaction.trade_date
                });
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
        assert_eq!(instruments.len(), 6);

        for instrument in &instruments {
            let ledger = crate::db::transactions::ledger_for_instrument(&pool, instrument.id)
                .await
                .expect("ledger should load");
            assert!(ledger.len() >= 2);
            crate::domain::derive_position_performance(&ledger).expect("ledger should derive");

            let latest_price = crate::db::prices::find_latest_on_or_before(
                &pool,
                instrument.id,
                PRICE_PROVIDER,
                today,
            )
            .await
            .expect("price lookup should succeed");
            assert!(latest_price.is_some());

            let provider_symbol = crate::db::provider_symbols::find_by_instrument_provider(
                &pool,
                instrument.id,
                PRICE_PROVIDER,
            )
            .await
            .expect("provider symbol lookup should succeed")
            .expect("provider symbol should exist");
            assert!(provider_symbol.enabled);
        }

        for currency in ["USD", "EUR"] {
            let latest_fx = crate::db::fx_rates::find_latest_on_or_before(
                &pool,
                currency,
                BASE_CURRENCY,
                FX_PROVIDER,
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
        .with_demo_mode(true);

        let holdings = get_json(&state, "/api/holdings").await;
        assert_eq!(holdings.as_array().expect("holdings array").len(), 6);
        assert_no_missing_price_or_fx(&holdings);
        for holding in holdings.as_array().expect("holdings array") {
            assert_eq!(holding["base"]["status"], "available");
            assert_eq!(
                holding["valuation"]["market_value_base"]["status"],
                "available"
            );
        }

        let gains = get_json(&state, &format!("/api/gains?end_date={today}")).await;
        assert_eq!(gains["rows"].as_array().expect("gains rows").len(), 6);
        assert_no_missing_price_or_fx(&gains);

        let value_history = get_json(&state, "/api/portfolio/value-history").await;
        assert!(!value_history["points"]
            .as_array()
            .expect("value-history points")
            .is_empty());
        assert_no_missing_price_or_fx(&value_history);

        let instrument_id = holdings[0]["instrument"]["id"]
            .as_i64()
            .expect("instrument id");
        let prices = get_json(&state, &format!("/api/instruments/{instrument_id}/prices")).await;
        assert!(!prices["points"]
            .as_array()
            .expect("price-history points")
            .is_empty());
        assert_no_missing_price_or_fx(&prices);
    }

    async fn get_json(state: &crate::state::AppState, uri: &str) -> Value {
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
}
