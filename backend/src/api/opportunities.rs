use axum::extract::{Query, State};
use axum::Json;
use chrono::NaiveDate;
use serde::{Deserialize, Serialize};

use crate::api::conviction_target::{build_conviction_target, ConvictionTargetResponse};
use crate::api::error::ApiError;
use crate::api::instruments::InstrumentResponse;
use crate::api::valuation::{
    gain_percent_string, money_string, serialize_availability, serialize_freshness,
    staler_freshness, AvailabilityResponse, PriceSnapshotResponse,
};
use crate::api::valued_holdings::{load_valued_holdings, ValuedOpenHolding};
use crate::domain::{
    derive_targets, price_versus_average, recent_price_change, Availability,
    ConvictionTargetOutput, PriceVersusAverage, RecentChangeReason, RecentPriceChange,
    RecentWindow, TransactionKind, ValuationReason,
};
use crate::market_data::effective_prices;
use crate::state::AppState;

#[derive(Debug, Deserialize)]
pub struct OpportunitiesQuery {
    pub window: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct OpportunitiesResponse {
    pub period: OpportunityPeriod,
    pub rows: Vec<OpportunityRow>,
    pub data_revision: String,
}

#[derive(Debug, Serialize)]
pub struct OpportunityPeriod {
    pub window: &'static str,
    pub start_date: String,
    pub end_date: String,
}

#[derive(Debug, Serialize)]
pub struct OpportunityRow {
    pub instrument: InstrumentResponse,
    pub quantity: i64,
    pub latest_close: Option<PriceSnapshotResponse>,
    pub price_change: PriceChangeResponse,
    pub average_cost_native: String,
    pub price_vs_average_percent: AvailabilityResponse,
    pub below_average: Option<bool>,
    pub unrealized_gain_percent: AvailabilityResponse,
    pub unrealized_gain_freshness: Option<String>,
    pub conviction_target: ConvictionTargetResponse,
}

#[derive(Debug, Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum PriceChangeResponse {
    Available {
        start_date: String,
        start_close: String,
        end_date: String,
        end_close: String,
        change_percent: String,
    },
    Unavailable {
        reasons: Vec<&'static str>,
    },
}

impl From<RecentPriceChange> for PriceChangeResponse {
    fn from(change: RecentPriceChange) -> Self {
        match change {
            RecentPriceChange::Available {
                start_date,
                start_close,
                end_date,
                end_close,
                change_percent,
            } => Self::Available {
                start_date: start_date.to_string(),
                start_close: start_close.to_string(),
                end_date: end_date.to_string(),
                end_close: end_close.to_string(),
                change_percent: money_string(change_percent),
            },
            RecentPriceChange::Unavailable(reasons) => Self::Unavailable {
                reasons: reasons
                    .into_iter()
                    .map(RecentChangeReason::as_str)
                    .collect(),
            },
        }
    }
}

pub async fn handler(
    State(state): State<AppState>,
    Query(query): Query<OpportunitiesQuery>,
) -> Result<Json<OpportunitiesResponse>, ApiError> {
    let window = match query.window.as_deref() {
        None => RecentWindow::OneWeek,
        Some(value) => RecentWindow::from_query(value).ok_or_else(|| {
            ApiError::bad_request("invalid_window", "window must be 7d, 14d or 30d")
        })?,
    };
    let today = state.clock.today();
    let start_date = window.start_date(today);
    let holdings = load_valued_holdings(&state.pool, today).await?;
    let inputs: Vec<_> = holdings
        .iter()
        .map(ValuedOpenHolding::conviction_target_input)
        .collect();
    let targets = derive_targets(&inputs);
    let mut rows = Vec::new();

    for (holding, target) in holdings
        .iter()
        .zip(&targets)
        .filter(|(holding, _)| holding.position.quantity > 0)
    {
        let end = holding.latest_price_snapshot();
        let start = if end.is_some_and(|end| end.date > start_date) {
            effective_prices::effective_latest_on_or_before(
                &state.pool,
                holding.instrument.id,
                start_date,
            )
            .await
            .map_err(|error| log_start_close_failure(holding, window, start_date, error))?
        } else {
            None
        };
        let split_dates: Vec<_> = holding
            .ledger
            .iter()
            .filter(|transaction| transaction.kind == TransactionKind::Split)
            .map(|transaction| transaction.trade_date)
            .collect();
        let change = if holding.valuation.is_some() {
            recent_price_change(start_date, end, start.as_ref(), &split_dates)
        } else {
            RecentPriceChange::Unavailable(vec![RecentChangeReason::NoPriceSource])
        };
        rows.push(build_opportunity_row(
            holding, target, change, window, start_date,
        )?);
    }

    Ok(Json(OpportunitiesResponse {
        period: OpportunityPeriod {
            window: window.as_str(),
            start_date: start_date.to_string(),
            end_date: today.to_string(),
        },
        rows,
        data_revision: state.revision.current().await.map_err(|error| {
            ApiError::internal(format!("could not read data revision: {error}"))
        })?,
    }))
}

fn build_opportunity_row(
    holding: &ValuedOpenHolding,
    target: &ConvictionTargetOutput,
    change: RecentPriceChange,
    window: RecentWindow,
    start_date: NaiveDate,
) -> Result<OpportunityRow, ApiError> {
    let average = holding.position.average_cost_native().ok_or_else(|| {
        log_internal_inconsistency(
            holding,
            window,
            start_date,
            "held position without average cost",
        )
    })?;
    let end = holding.latest_price_snapshot();
    let (price_vs_average_percent, below_average) =
        match price_versus_average(end, &holding.instrument.currency, Some(average)) {
            PriceVersusAverage::Available {
                percent,
                below_average,
            } => (
                AvailabilityResponse::Available {
                    value: money_string(percent),
                },
                Some(below_average),
            ),
            PriceVersusAverage::Unavailable(reasons) => (
                AvailabilityResponse::Unavailable {
                    reasons: reasons
                        .into_iter()
                        .map(|reason| reason.as_str().to_owned())
                        .collect(),
                },
                None,
            ),
        };
    let unrealized_gain_percent = match &holding.valuation {
        Some(valuation) => {
            serialize_availability(&valuation.unrealized_gain_percent, gain_percent_string)
        }
        None => serialize_availability(
            &Availability::<rust_decimal::Decimal>::unavailable(ValuationReason::MissingPrice),
            gain_percent_string,
        ),
    };
    // Missing snapshots carry no freshness claim; the gain's availability
    // already explains the missing price or FX.
    let unrealized_gain_freshness = holding
        .latest_price_snapshot()
        .zip(holding.latest_fx_snapshot())
        .map(|(price, fx)| serialize_freshness(staler_freshness(price.freshness, fx.freshness)));
    Ok(OpportunityRow {
        instrument: InstrumentResponse::from_row(&holding.instrument).map_err(|error| {
            log_internal_inconsistency(
                holding,
                window,
                start_date,
                &format!("invalid instrument response: {error:?}"),
            )
        })?,
        quantity: holding.position.quantity,
        latest_close: end.map(|snapshot| PriceSnapshotResponse {
            date: snapshot.date.to_string(),
            close: snapshot.close.to_string(),
            currency: snapshot.currency.clone(),
            source: snapshot.source.as_str().to_owned(),
            freshness: serialize_freshness(snapshot.freshness),
        }),
        price_change: change.into(),
        average_cost_native: money_string(average),
        price_vs_average_percent,
        below_average,
        unrealized_gain_percent,
        unrealized_gain_freshness,
        conviction_target: build_conviction_target(target, &holding.market_value_reasons()),
    })
}

fn log_start_close_failure(
    holding: &ValuedOpenHolding,
    window: RecentWindow,
    start_date: NaiveDate,
    error: crate::db::RepoError,
) -> ApiError {
    crate::engine_error!(
        "opportunities start-close lookup failed for instrument {} ({} / {}), window={} start_date={}: {}",
        holding.instrument.id,
        holding.instrument.exchange,
        holding.instrument.symbol,
        window.as_str(),
        start_date,
        error
    );
    ApiError::from(error)
}

fn log_internal_inconsistency(
    holding: &ValuedOpenHolding,
    window: RecentWindow,
    start_date: NaiveDate,
    reason: &str,
) -> ApiError {
    crate::engine_error!(
        "opportunities row inconsistency for instrument {} ({} / {}), window={} start_date={}: {}",
        holding.instrument.id,
        holding.instrument.exchange,
        holding.instrument.symbol,
        window.as_str(),
        start_date,
        reason
    );
    ApiError::internal(format!(
        "opportunities row inconsistency for instrument {}",
        holding.instrument.id
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::test_support::{instrument, send, set_conviction};
    use crate::clock::Clock;
    use crate::db::{fx_rates, prices, provider_symbols};
    use crate::providers::{MarketDataProvider, BASE_FX_PROVIDER};
    use axum::http::StatusCode;
    use rust_decimal_macros::dec;
    use serde_json::{json, Value};

    fn date(value: &str) -> NaiveDate {
        value.parse().expect("date")
    }

    async fn state() -> AppState {
        AppState::for_tests()
            .await
            .with_clock(Clock::fixed(date("2026-10-15")))
    }

    async fn trade(
        state: &AppState,
        id: i64,
        kind: &str,
        day: &str,
        quantity: i64,
        price: &str,
        currency: &str,
    ) {
        let (status, body) = send(state, "POST", "/api/transactions", json!({
            "instrument_id": id, "type": kind, "trade_date": day, "quantity": quantity,
            "price": price, "currency": currency, "fx_rate_to_base": if currency == "SEK" { "1" } else { "10" },
            "brokerage": "2.50",
        })).await;
        assert_eq!(status, StatusCode::CREATED, "{body}");
    }

    async fn add_prices(
        state: &AppState,
        id: i64,
        provider: MarketDataProvider,
        currency: &str,
        closes: &[(&str, &str)],
    ) {
        let now = "2026-10-15T12:00:00Z".to_owned();
        provider_symbols::upsert(
            &state.pool,
            &provider_symbols::NewProviderSymbol {
                instrument_id: id,
                provider,
                provider_symbol: format!("TEST-{id}"),
                asset_class: (provider == MarketDataProvider::NasdaqNordic)
                    .then(|| "SHARES".into()),
                currency: Some(currency.into()),
                enabled: true,
                created_at: now.clone(),
                updated_at: now.clone(),
            },
        )
        .await
        .expect("mapping");
        for (day, close) in closes {
            prices::upsert(
                &state.pool,
                &prices::NewPrice {
                    instrument_id: id,
                    provider,
                    provider_symbol: format!("TEST-{id}"),
                    date: date(day),
                    close: close.parse().expect("close"),
                    currency: currency.into(),
                    fetched_at: now.clone(),
                },
            )
            .await
            .expect("close");
        }
    }

    async fn add_nasdaq_prices(state: &AppState, id: i64, closes: &[(&str, &str)]) {
        add_prices(state, id, MarketDataProvider::NasdaqNordic, "SEK", closes).await;
    }

    async fn seed(
        state: &AppState,
        symbol: &str,
        currency: &str,
        average: &str,
        closes: &[(&str, &str)],
    ) -> i64 {
        let id = instrument(
            state,
            symbol,
            if currency == "SEK" { "STO" } else { "NASDAQ" },
            currency,
        )
        .await;
        trade(state, id, "Buy", "2026-08-01", 10, average, currency).await;
        add_prices(state, id, MarketDataProvider::Yahoo, currency, closes).await;
        set_conviction(state, id, "High").await;
        id
    }

    async fn fx(state: &AppState, day: &str, rate: &str) {
        fx_rates::upsert(
            &state.pool,
            &fx_rates::NewFxRate {
                base: "USD".into(),
                quote: "SEK".into(),
                date: date(day),
                rate: rate.parse().expect("rate"),
                provider: BASE_FX_PROVIDER,
                fetched_at: "2026-10-15T12:00:00Z".into(),
            },
        )
        .await
        .expect("fx rate");
    }

    async fn get(state: &AppState, window: &str) -> Value {
        let (status, body) = send(
            state,
            "GET",
            &format!("/api/opportunities?window={window}"),
            Value::Null,
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{body}");
        body
    }

    fn row<'a>(body: &'a Value, symbol: &str) -> &'a Value {
        body["rows"]
            .as_array()
            .expect("rows")
            .iter()
            .find(|row| row["instrument"]["symbol"] == symbol)
            .expect("row")
    }

    fn assert_reason(row: &Value, reason: &str) {
        assert_eq!(
            row["price_change"],
            json!({ "status": "unavailable", "reasons": [reason] })
        );
    }

    #[tokio::test]
    async fn default_week_uses_the_clock_and_reports_actual_full_precision_closes_and_revision() {
        let state = state().await;
        seed(
            &state,
            "FALL",
            "SEK",
            "398.20",
            &[
                ("2026-10-07", "412.512345"),
                ("2026-10-14", "377.812345"),
                ("2026-10-16", "999"), // future data must never become the end close
            ],
        )
        .await;
        let body = get(&state, "7d").await;
        assert_eq!(
            body["period"],
            json!({"window":"7d", "start_date":"2026-10-08", "end_date":"2026-10-15"})
        );
        let holding = row(&body, "FALL");
        assert_eq!(holding["quantity"], 10);
        assert_eq!(holding["average_cost_native"], "398.20");
        assert_eq!(
            holding["price_change"],
            json!({
                "status":"available", "start_date":"2026-10-07", "start_close":"412.512345",
                "end_date":"2026-10-14", "end_close":"377.812345", "change_percent":"-8.41",
            })
        );
        assert_eq!(holding["latest_close"]["close"], "377.812345");
        assert_eq!(holding["latest_close"]["date"], "2026-10-14");
        assert_eq!(holding["latest_close"]["currency"], "SEK");
        assert_eq!(holding["latest_close"]["source"], "YAHOO");
        assert_eq!(holding["latest_close"]["freshness"], "minor_stale_1_days");
        assert_eq!(
            body["data_revision"],
            state.revision.current().await.expect("revision")
        );
        let (status, default) = send(&state, "GET", "/api/opportunities", Value::Null).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(default, body);
    }

    #[tokio::test]
    async fn each_window_resolves_its_own_start_close_and_invalid_windows_are_rejected() {
        let state = state().await;
        seed(
            &state,
            "WINDOWS",
            "SEK",
            "100",
            &[
                ("2026-09-15", "200"),
                ("2026-10-01", "125"),
                ("2026-10-08", "100"),
                ("2026-10-15", "90"),
            ],
        )
        .await;
        for (window, start, close, change) in [
            ("7d", "2026-10-08", "100", "-10.00"),
            ("14d", "2026-10-01", "125", "-28.00"),
            ("30d", "2026-09-15", "200", "-55.00"),
        ] {
            let body = get(&state, window).await;
            assert_eq!(body["period"]["window"], window);
            assert_eq!(body["period"]["start_date"], start);
            assert_eq!(row(&body, "WINDOWS")["price_change"]["start_date"], start);
            assert_eq!(row(&body, "WINDOWS")["price_change"]["start_close"], close);
            assert_eq!(
                row(&body, "WINDOWS")["price_change"]["change_percent"],
                change
            );
        }
        for invalid in ["bogus", "7D", "", "1w", "%207d", "7d%20"] {
            let (status, body) = send(
                &state,
                "GET",
                &format!("/api/opportunities?window={invalid}"),
                Value::Null,
            )
            .await;
            assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
            assert_eq!(body["error"]["code"], "invalid_window");
        }
    }

    #[tokio::test]
    async fn excessive_start_gap_is_unavailable_but_latest_close_stays_visible() {
        let state = state().await;
        seed(
            &state,
            "GAP",
            "SEK",
            "100",
            &[("2026-10-02", "100"), ("2026-10-15", "90")],
        )
        .await;
        let body = get(&state, "7d").await;
        assert_reason(row(&body, "GAP"), "start_close_too_old");
        assert_eq!(row(&body, "GAP")["latest_close"]["close"], "90");
    }

    #[tokio::test]
    async fn cross_source_pair_is_unavailable_and_nasdaq_only_pair_is_available() {
        let state = state().await;
        let mixed = seed(&state, "MIXED", "SEK", "100", &[("2026-10-08", "100")]).await;
        add_nasdaq_prices(&state, mixed, &[("2026-10-15", "90")]).await;
        let nordic = seed(&state, "NORDIC", "SEK", "100", &[]).await;
        add_nasdaq_prices(
            &state,
            nordic,
            &[("2026-10-08", "120"), ("2026-10-15", "90")],
        )
        .await;
        let body = get(&state, "7d").await;
        assert_reason(row(&body, "MIXED"), "start_close_source_mismatch");
        assert_eq!(
            row(&body, "MIXED")["latest_close"]["source"],
            "NASDAQ_NORDIC"
        );
        assert_eq!(
            row(&body, "NORDIC")["price_change"]["change_percent"],
            "-25.00"
        );
    }

    #[tokio::test]
    async fn old_latest_and_single_close_never_report_zero_and_keep_stale_price_visible() {
        let state = state().await;
        for (symbol, day) in [("OLD", "2026-10-01"), ("ONSTART", "2026-10-08")] {
            seed(&state, symbol, "SEK", "100", &[(day, "100")]).await;
        }
        let body = get(&state, "7d").await;
        for symbol in ["OLD", "ONSTART"] {
            let holding = row(&body, symbol);
            assert_reason(holding, "no_close_since_period_start");
            assert_eq!(holding["latest_close"]["close"], "100");
            assert!(holding["latest_close"]["freshness"]
                .as_str()
                .expect("freshness")
                .starts_with("warning_stale_"));
        }
    }

    #[tokio::test]
    async fn held_only_rows_remain_in_instrument_order_and_unmapped_rows_remain_visible() {
        let state = state().await;
        seed(
            &state,
            "ZZZ",
            "SEK",
            "100",
            &[("2026-10-08", "100"), ("2026-10-15", "110")],
        )
        .await;
        let watch = seed(&state, "WATCH", "SEK", "100", &[("2026-10-15", "100")]).await;
        trade(&state, watch, "Sell", "2026-09-01", 10, "100", "SEK").await;
        let closed = seed(&state, "CLOSED", "SEK", "100", &[("2026-10-15", "100")]).await;
        set_conviction(&state, closed, "Other").await;
        trade(&state, closed, "Sell", "2026-09-01", 10, "100", "SEK").await;
        let unmapped = instrument(&state, "AAA", "STO", "SEK").await;
        trade(&state, unmapped, "Buy", "2026-08-01", 10, "100", "SEK").await;
        let body = get(&state, "7d").await;
        let symbols: Vec<_> = body["rows"]
            .as_array()
            .expect("rows")
            .iter()
            .map(|row| row["instrument"]["symbol"].as_str().expect("symbol"))
            .collect();
        assert_eq!(symbols, ["AAA", "ZZZ"]);
        assert_reason(row(&body, "AAA"), "no_price_source");
        assert!(row(&body, "AAA")["latest_close"].is_null());
        assert!(row(&body, "AAA")["below_average"].is_null());
        assert!(row(&body, "AAA")["unrealized_gain_freshness"].is_null());
    }

    #[tokio::test]
    async fn native_change_ignores_fx_movement_and_average_comparison_excludes_fees() {
        let state = state().await;
        seed(
            &state,
            "BELOW",
            "USD",
            "100",
            &[("2026-10-08", "100"), ("2026-10-15", "90")],
        )
        .await;
        seed(
            &state,
            "ABOVE",
            "SEK",
            "100",
            &[("2026-10-08", "100"), ("2026-10-15", "110")],
        )
        .await;
        fx(&state, "2026-10-08", "10").await;
        fx(&state, "2026-10-15", "20").await;
        let body = get(&state, "7d").await;
        let below = row(&body, "BELOW");
        assert_eq!(below["price_change"]["change_percent"], "-10.00");
        assert_eq!(
            below["price_vs_average_percent"],
            json!({"status":"available", "value":"-10.00"})
        );
        assert_eq!(below["below_average"], true);
        assert_eq!(below["unrealized_gain_percent"]["value"], "79.95");
        let above = row(&body, "ABOVE");
        assert_eq!(above["price_vs_average_percent"]["value"], "10.00");
        assert_eq!(above["below_average"], false);
    }

    #[tokio::test]
    async fn gains_and_targets_equal_holdings_with_hidden_watchlist_pool_members() {
        let state = state().await;
        seed(
            &state,
            "AAA",
            "SEK",
            "100",
            &[("2026-10-08", "100"), ("2026-10-15", "90")],
        )
        .await;
        seed(
            &state,
            "BBB",
            "USD",
            "100",
            &[("2026-10-08", "100"), ("2026-10-15", "110")],
        )
        .await;
        // The 2.50 SEK fee makes the basis 1002.50; a value of 1002.49
        // produces a gain near -0.001%, which Holdings prints as -0.00.
        seed(
            &state,
            "TINYGAIN",
            "SEK",
            "100",
            &[("2026-10-08", "100.25"), ("2026-10-15", "100.249")],
        )
        .await;
        seed(&state, "MISSING", "SEK", "100", &[]).await;
        let watch = seed(&state, "WATCH", "SEK", "100", &[("2026-10-15", "100")]).await;
        set_conviction(&state, watch, "Low").await;
        trade(&state, watch, "Sell", "2026-09-01", 10, "100", "SEK").await;
        fx(&state, "2026-10-15", "10").await;
        let body = get(&state, "7d").await;
        let (status, holdings) = send(&state, "GET", "/api/holdings", Value::Null).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(holdings["hidden_watchlist_pool_count"], 1);
        assert_eq!(
            row(&body, "TINYGAIN")["unrealized_gain_percent"],
            json!({"status": "available", "value": "-0.00"})
        );
        let holdings = holdings["holdings"].as_array().expect("holdings");
        assert_eq!(body["rows"].as_array().expect("rows").len(), holdings.len());
        for opportunity in body["rows"].as_array().expect("rows") {
            let holding = holdings
                .iter()
                .find(|holding| holding["instrument"]["id"] == opportunity["instrument"]["id"])
                .expect("holding");
            assert_eq!(
                opportunity["unrealized_gain_percent"],
                holding["valuation"]["unrealized_gain_percent"]
            );
            assert_eq!(
                opportunity["conviction_target"],
                holding["conviction_target"]
            );
        }
    }

    #[tokio::test]
    async fn fresh_price_and_stale_fx_keep_gain_available_and_report_separate_freshness() {
        let state = state().await;
        seed(&state, "USD", "USD", "100", &[("2026-10-15", "110")]).await;
        seed(&state, "SEK", "SEK", "100", &[("2026-10-15", "110")]).await;
        fx(&state, "2026-10-05", "10").await;
        let id = instrument(&state, "UNMAPPED", "STO", "SEK").await;
        trade(&state, id, "Buy", "2026-08-01", 10, "100", "SEK").await;
        let body = get(&state, "7d").await;
        let usd = row(&body, "USD");
        assert_eq!(usd["unrealized_gain_percent"]["status"], "available");
        assert_eq!(usd["latest_close"]["freshness"], "fresh");
        assert_eq!(usd["unrealized_gain_freshness"], "warning_stale_8_days");
        assert_eq!(row(&body, "SEK")["unrealized_gain_freshness"], "fresh");
        assert!(row(&body, "UNMAPPED")["unrealized_gain_freshness"].is_null());
    }

    #[tokio::test]
    async fn ledger_split_inside_actual_close_interval_blocks_change() {
        let state = state().await;
        let id = seed(
            &state,
            "SPLIT",
            "SEK",
            "100",
            &[("2026-10-07", "100"), ("2026-10-15", "50")],
        )
        .await;
        let (status, body) = send(
            &state,
            "POST",
            "/api/transactions",
            json!({
                "instrument_id":id, "type":"Split", "trade_date":"2026-10-08", "quantity":10,
            }),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED, "{body}");
        let body = get(&state, "7d").await;
        assert_reason(row(&body, "SPLIT"), "split_in_period");
        assert_eq!(row(&body, "SPLIT")["quantity"], 20);
        assert_eq!(row(&body, "SPLIT")["average_cost_native"], "50.00");
    }

    #[tokio::test]
    async fn missing_closes_currency_mismatch_and_zero_start_have_explicit_reasons() {
        let state = state().await;
        seed(&state, "EMPTY", "SEK", "100", &[]).await;
        seed(&state, "NOSTART", "SEK", "100", &[("2026-10-15", "90")]).await;
        seed(
            &state,
            "ZERO",
            "SEK",
            "100",
            &[("2026-10-08", "0"), ("2026-10-15", "90")],
        )
        .await;
        let mismatch = seed(&state, "CURRENCY", "SEK", "100", &[("2026-10-08", "100")]).await;
        add_prices(
            &state,
            mismatch,
            MarketDataProvider::Yahoo,
            "USD",
            &[("2026-10-15", "90")],
        )
        .await;
        let body = get(&state, "7d").await;
        assert_reason(row(&body, "EMPTY"), "missing_latest_close");
        assert!(row(&body, "EMPTY")["latest_close"].is_null());
        assert_reason(row(&body, "NOSTART"), "missing_start_close");
        assert_reason(row(&body, "ZERO"), "zero_start_close");
        assert_reason(row(&body, "CURRENCY"), "start_close_currency_mismatch");
        assert_eq!(
            row(&body, "CURRENCY")["price_vs_average_percent"],
            json!({"status":"unavailable", "reasons":["price_currency_mismatch"]})
        );
        assert!(row(&body, "CURRENCY")["below_average"].is_null());
    }

    #[tokio::test]
    async fn percentages_normalize_negative_zero_without_losing_below_average_flag() {
        let state = state().await;
        seed(
            &state,
            "TINY",
            "SEK",
            "100.00000001",
            &[("2026-10-08", "100.00000001"), ("2026-10-15", "100")],
        )
        .await;
        let body = get(&state, "7d").await;
        let tiny = row(&body, "TINY");
        assert_eq!(tiny["price_change"]["change_percent"], "0.00");
        assert_eq!(tiny["price_vs_average_percent"]["value"], "0.00");
        assert_eq!(tiny["below_average"], true);
    }

    #[test]
    fn exact_domain_change_serializes_small_negative_as_zero() {
        let response = PriceChangeResponse::from(RecentPriceChange::Available {
            start_date: date("2026-10-08"),
            start_close: dec!(100),
            end_date: date("2026-10-15"),
            end_close: dec!(99.999),
            change_percent: dec!(-0.001),
        });
        assert_eq!(
            serde_json::to_value(response).expect("json")["change_percent"],
            "0.00"
        );
    }

    #[tokio::test]
    async fn demo_mode_serves_available_changes_for_every_held_row_in_every_window() {
        let state = state().await.with_mode(crate::config::Mode::Demo);
        crate::demo::seed(&state.pool, state.clock.today())
            .await
            .expect("demo data");
        let (status, holdings) = send(&state, "GET", "/api/holdings", Value::Null).await;
        assert_eq!(status, StatusCode::OK);
        let holdings = holdings["holdings"].as_array().expect("holdings");
        assert!(!holdings.is_empty());
        for window in ["7d", "14d", "30d"] {
            let body = get(&state, window).await;
            let rows = body["rows"].as_array().expect("rows");
            assert_eq!(rows.len(), holdings.len());
            assert!(body["data_revision"].is_string());
            for holding in holdings {
                let opportunity = rows
                    .iter()
                    .find(|row| row["instrument"]["id"] == holding["instrument"]["id"])
                    .expect("held demo row");
                assert_eq!(
                    opportunity["price_change"]["status"], "available",
                    "window={window}, row={opportunity}"
                );
                assert_eq!(
                    opportunity["unrealized_gain_percent"],
                    holding["valuation"]["unrealized_gain_percent"]
                );
                assert_eq!(
                    opportunity["conviction_target"],
                    holding["conviction_target"]
                );
            }
        }
    }
}
