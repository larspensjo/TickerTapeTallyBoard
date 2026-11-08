use super::*;
use crate::{
    api::{
        test_support::{seed_valued_at, send, SeededHoldingSpec},
        valued_holdings::load_valued_holdings,
    },
    clock::Clock,
    config::Mode,
    db::{
        fundamental_values::FetchWrite,
        fundamentals_sources::{GenerationWrite, LinkDefinition, LinkOrigin, PreviousLink},
        prices,
    },
    domain::fundamentals::{FundamentalValue, FundamentalsSource},
    providers::MarketDataProvider,
};
use axum::http::StatusCode;
use chrono::Duration;
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use serde_json::Value;

fn date(s: &str) -> NaiveDate {
    s.parse().unwrap()
}
async fn demo() -> AppState {
    let today = date("2026-11-07");
    let state = AppState::for_tests()
        .await
        .with_clock(Clock::fixed(today))
        .with_mode(Mode::Demo);
    crate::demo::seed(&state.pool, today).await.unwrap();
    state
}
async fn get(state: &AppState, uri: &str) -> Value {
    let (status, body) = send(state, "GET", uri, Value::Null).await;
    assert_eq!(status, StatusCode::OK, "{uri}: {body}");
    body
}
fn without_fundamentals(mut row: Value) -> Vec<u8> {
    row.as_object_mut().unwrap().remove("fundamentals");
    serde_json::to_vec(&row).unwrap()
}
fn assert_figure(f: &Value) {
    let certainty = f["certainty"].as_str().unwrap();
    assert!(f["reasons"].is_array());
    if matches!(certainty, "complete" | "approximate") {
        f["value"].as_str().unwrap().parse::<Decimal>().unwrap();
        if certainty == "approximate" {
            assert!(!f["reasons"].as_array().unwrap().is_empty());
        }
    } else {
        assert!(f["value"].is_null());
        assert!(!f["reasons"].as_array().unwrap().is_empty());
    }
}

#[tokio::test]
async fn offline_query_only_demo_endpoints_expose_every_figure_state_and_source_panel() {
    let state = demo().await;
    crate::db::testing::set_query_only(&state.pool).await;
    let holdings = get(&state, "/api/holdings").await;
    assert!(holdings["data_revision"].is_string());
    let mut certainties = std::collections::BTreeSet::new();
    let mut future = false;
    let mut passed = false;
    for row in holdings["holdings"].as_array().unwrap() {
        let id = row["instrument"]["id"].as_i64().unwrap();
        let symbol = row["instrument"]["symbol"].as_str().unwrap();
        let f = get(&state, &format!("/api/instruments/{id}/fundamentals")).await;
        assert_eq!(f["data_revision"], holdings["data_revision"]);
        for metric in ["pe", "ps", "profit_margin", "sales_growth"] {
            assert_figure(&f[metric]);
            certainties.insert(f[metric]["certainty"].as_str().unwrap().to_owned());
        }
        for quarter in f["quarters"].as_array().unwrap() {
            assert!(quarter.get("net_income").is_none());
            assert!(quarter.get("shares").is_none());
        }
        for field in [
            "symbol",
            "origin",
            "security_type",
            "classification",
            "status",
            "candidates",
            "last_success",
            "last_attempt_at",
            "last_attempt_outcome",
            "last_attempt_reason",
            "awaiting_figures",
        ] {
            assert!(f["source"].get(field).is_some(), "{symbol} {field}");
        }
        for metric in ["pe", "ps", "next_report"] {
            assert_eq!(row["fundamentals"][metric], f[metric], "{symbol} {metric}");
        }
        if let Some(days) = f["next_report"]["days_until"].as_i64() {
            future |= days > 0;
            passed |= days < 0;
            assert_eq!(
                days,
                (date(f["next_report"]["date"].as_str().unwrap()) - state.clock.today()).num_days()
            );
        }
        match symbol {
            "NOVA" => {
                assert_eq!(f["pe"]["certainty"], "complete");
                assert_eq!(f["last_report"]["certainty"], "observed");
                assert!(f["fx_conversion"].is_null());
            }
            "NORD" => {
                assert_eq!(f["pe"]["certainty"], "complete");
                assert_eq!(f["fx_conversion"]["from"], "EUR");
                assert_eq!(f["fx_conversion"]["to"], "SEK");
                assert!(f["fx_conversion"]["rate"].is_string());
            }
            "SKOG" => {
                assert_eq!(f["pe"]["certainty"], "approximate");
                assert_eq!(f["ps"]["certainty"], "approximate");
                assert!(f["quarters"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|q| q["missing"] == true && q["sales"]["value"].is_null()));
                assert!(f["quarters"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|q| q["eps"]["reasons"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .any(|r| r["code"] == "eps_derived_from_net_income")));
                assert_eq!(f["next_report"]["reasons"][0]["code"], "estimate_passed");
            }
            "ALBA" => {
                assert!(f["pe"]["value"].is_null());
                assert_eq!(f["pe"]["reasons"][0]["code"], "loss_making");
                assert_eq!(f["ps"]["certainty"], "complete");
            }
            "GLBL" => {
                assert_eq!(f["pe"]["certainty"], "not_applicable");
                assert_eq!(f["next_report"]["certainty"], "not_applicable");
            }
            "VARV" => {
                assert_eq!(f["pe"]["reasons"][0]["code"], "no_fundamentals_source");
            }
            "HARV" => {
                assert_eq!(f["share_basis"]["state"], "listing_unit_mismatch");
                assert_eq!(
                    f["share_basis"]["ratio"]
                        .as_str()
                        .unwrap()
                        .parse::<Decimal>()
                        .unwrap(),
                    dec!(0.2)
                );
                assert_eq!(f["profit_margin"]["certainty"], "complete");
            }
            _ => {}
        }
    }
    assert_eq!(
        certainties,
        ["complete", "approximate", "not_applicable", "unavailable"]
            .into_iter()
            .map(str::to_owned)
            .collect()
    );
    assert!(future && passed);
    let (status, _) = send(
        &state,
        "GET",
        "/api/instruments/999999/fundamentals",
        Value::Null,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, _) = send(
        &state,
        "POST",
        "/api/prices/refresh",
        serde_json::json!({"mode":"latest"}),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn holdings_assembly_reuses_loaded_close_even_when_the_price_table_changes() {
    let state = demo().await;
    let baseline = get(&state, "/api/holdings").await;
    let loaded = load_valued_holdings(&state.pool, state.clock.today())
        .await
        .unwrap();
    let nova = loaded
        .iter()
        .find(|h| h.instrument.symbol == "NOVA")
        .unwrap();
    let old = nova.latest_price_snapshot().unwrap();
    // Inject a conflicting current close after valuation loading. Any second read
    // for either the ratio or same-date unit evidence makes the contract fail.
    prices::upsert(
        &state.pool,
        &prices::NewPrice {
            instrument_id: nova.instrument.id,
            provider: MarketDataProvider::Yahoo,
            provider_symbol: "NOVA".into(),
            date: old.date,
            close: old.close * dec!(2),
            currency: old.currency.clone(),
            fetched_at: format!("{}T13:00:00Z", state.clock.today()),
        },
    )
    .await
    .unwrap();
    let (rows, _) = crate::api::holdings::build_holdings(
        &state.pool,
        &state.clock,
        state.clock.today(),
        loaded,
        false,
    )
    .await
    .unwrap();
    for row in rows {
        let row = serde_json::to_value(row).unwrap();
        let original = baseline["holdings"]
            .as_array()
            .unwrap()
            .iter()
            .find(|r| r["instrument"]["id"] == row["instrument"]["id"])
            .unwrap();
        assert_eq!(
            without_fundamentals(row.clone()),
            without_fundamentals(original.clone())
        );
        assert_eq!(row["fundamentals"], original["fundamentals"]);
    }
    let changed = get(&state, "/api/holdings").await;
    assert_ne!(
        changed["holdings"], baseline["holdings"],
        "injected table value is observable on the next valuation load"
    );
}

async fn seed_company(state: &AppState, id: i64) {
    seed_company_with_currencies(state, id, "SEK", "SEK").await;
}

async fn seed_company_with_currencies(state: &AppState, id: i64, report: &str, listing: &str) {
    let today = state.clock.today();
    let mut values = Vec::new();
    for end in [
        "2025-09-30",
        "2025-12-31",
        "2026-03-31",
        "2026-06-30",
        "2026-09-30",
    ] {
        for (metric, value) in [
            (Metric::Revenue, dec!(1000)),
            (Metric::DilutedEps, dec!(1)),
            (Metric::DilutedShares, dec!(100)),
            (Metric::DilutedNetIncome, dec!(100)),
        ] {
            values.push(FundamentalValue {
                period_type: PeriodType::Quarter,
                period_end: date(end),
                metric,
                value,
                currency: Some(report.into()),
            });
        }
    }
    values.push(FundamentalValue {
        period_type: PeriodType::Evidence,
        period_end: today,
        metric: Metric::MarketCap,
        value: dec!(10000),
        currency: Some(listing.into()),
    });
    let mut tx = state.pool.begin().await.unwrap();
    let write = FetchWrite {
        instrument_id: id,
        expected: None,
        verified: None,
        discovery: Some(LinkDefinition {
            source: Some(FundamentalsSource::Yahoo),
            symbol: Some("TEST".into()),
            security_type: Some("EQUITY".into()),
            classification: Classification::Equity,
            origin: LinkOrigin::Discovered,
            status: LinkStatus::Usable,
            candidates: "[]".into(),
        }),
        values,
        splits: vec![],
        at: format!("{today}T00:00:00Z"),
        seen_on: today,
        previous_success_on: None,
        status: AttemptStatus::Succeeded,
        reason: None,
    };
    assert!(matches!(
        fundamental_values::write_fetch(&mut tx, &write)
            .await
            .unwrap(),
        GenerationWrite::Applied(_)
    ));
    tx.commit().await.unwrap();
}

#[tokio::test]
async fn fundamentals_are_additive_and_corrections_exclude_all_set_aside_history() {
    let today = date("2026-11-07");
    let state = AppState::for_tests().await.with_clock(Clock::fixed(today));
    let id = seed_valued_at(
        &state,
        SeededHoldingSpec {
            symbol: "TEST",
            exchange: "STO",
            currency: "SEK",
            quantity: 10,
            price: "100",
            conviction: "High",
            price_date: today,
            fx_date: None,
            fx_rate: None,
        },
    )
    .await;
    let before = get(&state, "/api/holdings").await;
    seed_company(&state, id).await;
    let after = get(&state, "/api/holdings").await;
    assert_eq!(
        without_fundamentals(before["holdings"][0].clone()),
        without_fundamentals(after["holdings"][0].clone())
    );
    assert_eq!(after["holdings"][0]["fundamentals"]["pe"]["value"], "25");
    let old = fundamentals_sources::find(&state.pool, id)
        .await
        .unwrap()
        .unwrap();
    fundamentals_sources::correct(
        &state.pool,
        id,
        Some(old.definition.clone()),
        Some(PreviousLink::SameCompany),
        &format!("{today}T01:00:00Z"),
    )
    .await
    .unwrap();
    let retained = get(&state, &format!("/api/instruments/{id}/fundamentals")).await;
    assert_eq!(retained["pe"]["value"], "25");
    assert_eq!(retained["source"]["awaiting_figures"], false);
    fundamentals_sources::correct(
        &state.pool,
        id,
        None,
        Some(PreviousLink::SameCompany),
        &format!("{today}T01:30:00Z"),
    )
    .await
    .unwrap();
    let pending = get(&state, &format!("/api/instruments/{id}/fundamentals")).await;
    assert_eq!(pending["pe"]["value"], "25");
    assert_eq!(pending["source"]["status"], "PENDING");
    fundamentals_sources::correct(
        &state.pool,
        id,
        Some(old.definition),
        Some(PreviousLink::WrongCompany),
        &format!("{today}T02:00:00Z"),
    )
    .await
    .unwrap();
    let response = get(&state, &format!("/api/instruments/{id}/fundamentals")).await;
    assert_eq!(response["source"]["awaiting_figures"], true);
    assert!(response["quarters"].as_array().unwrap().is_empty());
    assert_eq!(response["pe"]["reasons"][0]["code"], "awaiting_figures");
    assert_eq!(
        response["next_report"]["reasons"][0]["code"],
        "awaiting_figures"
    );
    assert!(response["last_report"]["period_end"].is_null());
    let holdings = get(&state, "/api/holdings").await;
    assert_eq!(
        holdings["holdings"][0]["fundamentals"]["pe"]["reasons"][0]["code"],
        "awaiting_figures"
    );
    finish_attempt(&state, id, AttemptStatus::Empty).await;
    let empty = get(&state, &format!("/api/instruments/{id}/fundamentals")).await;
    assert_eq!(empty["source"]["awaiting_figures"], false);
    assert_eq!(empty["source"]["last_attempt_outcome"], "EMPTY");
    assert_eq!(empty["pe"]["reasons"][0]["code"], "too_few_quarters");
    assert_eq!(
        empty["next_report"]["reasons"][0]["code"],
        "too_few_quarters"
    );
    assert!(empty["quarters"].as_array().unwrap().is_empty());
    let holdings = get(&state, "/api/holdings").await;
    assert_eq!(holdings["holdings"][0]["fundamentals"]["pe"], empty["pe"]);
}

async fn finish_attempt(state: &AppState, id: i64, status: AttemptStatus) {
    let link = fundamentals_sources::find(&state.pool, id)
        .await
        .unwrap()
        .unwrap();
    let mut tx = state.pool.begin().await.unwrap();
    assert!(matches!(
        fundamental_values::write_fetch(
            &mut tx,
            &FetchWrite {
                instrument_id: id,
                expected: Some(link.token),
                discovery: None,
                verified: None,
                values: vec![],
                splits: vec![],
                at: format!("{}T14:00:00Z", state.clock.today()),
                seen_on: state.clock.today(),
                previous_success_on: None,
                status,
                reason: (status == AttemptStatus::Failed)
                    .then(|| "temporary source failure".into()),
            }
        )
        .await
        .unwrap(),
        GenerationWrite::Applied(_)
    ));
    tx.commit().await.unwrap();
}

#[tokio::test]
async fn failed_attempts_stay_in_the_panel_until_last_good_figures_are_stale() {
    let state = demo().await;
    let nova = instruments::list(&state.pool)
        .await
        .unwrap()
        .into_iter()
        .find(|i| i.symbol == "NOVA")
        .unwrap();
    let uri = format!("/api/instruments/{}/fundamentals", nova.id);
    finish_attempt(&state, nova.id, AttemptStatus::Failed).await;
    let recent = get(&state, &uri).await;
    assert_eq!(recent["pe"]["certainty"], "complete");
    assert_eq!(recent["source"]["last_attempt_outcome"], "FAILED");
    assert_eq!(
        recent["source"]["last_attempt_reason"],
        "temporary source failure"
    );
    assert_eq!(recent["source"]["last_success"], "2026-11-07T12:00:00Z");
    let later = state
        .clone()
        .with_clock(Clock::fixed(state.clock.today() + Duration::days(7)));
    let seventh = get(&later, &uri).await;
    assert!(!seventh["pe"]["reasons"]
        .as_array()
        .unwrap()
        .iter()
        .any(|r| r["code"] == "source_unreachable"));
    let later = state
        .clone()
        .with_clock(Clock::fixed(state.clock.today() + Duration::days(8)));
    let stale = get(&later, &uri).await;
    assert_eq!(stale["pe"]["certainty"], "approximate");
    let outage = stale["pe"]["reasons"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["code"] == "source_unreachable")
        .unwrap();
    assert_eq!(outage["detail"], "2026-11-07");
    let holdings = get(&later, "/api/holdings").await;
    let row = holdings["holdings"]
        .as_array()
        .unwrap()
        .iter()
        .find(|h| h["instrument"]["id"] == nova.id)
        .unwrap();
    assert_eq!(row["fundamentals"]["pe"], stale["pe"]);
}

#[tokio::test]
async fn holdings_do_not_decode_panel_candidates_or_irrelevant_observations() {
    let state = demo().await;
    let nova = instruments::list(&state.pool)
        .await
        .unwrap()
        .into_iter()
        .find(|i| i.symbol == "NOVA")
        .unwrap();
    let before = get(&state, "/api/holdings").await;
    crate::db::testing::corrupt_fundamentals_panel_data(&state.pool, nova.id).await;
    let after = get(&state, "/api/holdings").await;
    let row = |body: &Value| {
        body["holdings"]
            .as_array()
            .unwrap()
            .iter()
            .find(|h| h["instrument"]["id"] == nova.id)
            .unwrap()["fundamentals"]
            .clone()
    };
    assert_eq!(row(&after), row(&before));
    let (status, _) = send(
        &state,
        "GET",
        &format!("/api/instruments/{}/fundamentals", nova.id),
        Value::Null,
    )
    .await;
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
}

#[tokio::test]
async fn holdings_assembly_reuses_the_loaded_ledger_without_decoding_it_again() {
    let state = demo().await;
    let baseline = get(&state, "/api/holdings").await;
    let loaded = load_valued_holdings(&state.pool, state.clock.today())
        .await
        .unwrap();
    let id = loaded
        .iter()
        .find(|h| h.instrument.symbol == "NOVA")
        .unwrap()
        .instrument
        .id;
    crate::db::testing::corrupt_ledger_price(&state.pool, id).await;
    let (rows, _) = crate::api::holdings::build_holdings(
        &state.pool,
        &state.clock,
        state.clock.today(),
        loaded,
        false,
    )
    .await
    .unwrap();
    let row = rows
        .into_iter()
        .map(|r| serde_json::to_value(r).unwrap())
        .find(|r| r["instrument"]["id"] == id)
        .unwrap();
    let original = baseline["holdings"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["instrument"]["id"] == id)
        .unwrap();
    assert_eq!(row, *original);
}

#[tokio::test]
async fn holdings_ratios_reuse_loaded_listing_fx_after_the_rate_table_changes() {
    let today = date("2026-11-07");
    let state = AppState::for_tests().await.with_clock(Clock::fixed(today));
    let id = seed_valued_at(
        &state,
        SeededHoldingSpec {
            symbol: "CROSS",
            exchange: "NYSE",
            currency: "USD",
            quantity: 10,
            price: "100",
            conviction: "High",
            price_date: today,
            fx_date: Some(today),
            fx_rate: Some("10"),
        },
    )
    .await;
    seed_company_with_currencies(&state, id, "EUR", "USD").await;
    let mut rate = fx_rates::NewFxRate {
        base: "EUR".into(),
        quote: "SEK".into(),
        date: today,
        rate: dec!(11),
        provider: BASE_FX_PROVIDER,
        fetched_at: format!("{today}T12:00:00Z"),
    };
    fx_rates::upsert(&state.pool, &rate).await.unwrap();
    let before = get(&state, "/api/holdings").await;
    let panel = get(&state, &format!("/api/instruments/{id}/fundamentals")).await;
    assert_eq!(
        panel["fx_conversion"]["rate"]
            .as_str()
            .unwrap()
            .parse::<Decimal>()
            .unwrap(),
        dec!(1.1)
    );
    let loaded = load_valued_holdings(&state.pool, today).await.unwrap();
    rate.base = "USD".into();
    rate.rate = dec!(20);
    fx_rates::upsert(&state.pool, &rate).await.unwrap();
    let (rows, _) =
        crate::api::holdings::build_holdings(&state.pool, &state.clock, today, loaded, false)
            .await
            .unwrap();
    let row = serde_json::to_value(&rows[0]).unwrap();
    assert_eq!(row, before["holdings"][0]);
    let after = get(&state, "/api/holdings").await;
    assert_ne!(
        after["holdings"][0]["fundamentals"]["pe"],
        row["fundamentals"]["pe"]
    );
}

#[tokio::test]
async fn report_days_follow_the_backend_clock_and_passed_estimates_are_retained() {
    let state = demo().await;
    let nova = instruments::list(&state.pool)
        .await
        .unwrap()
        .into_iter()
        .find(|i| i.symbol == "NOVA")
        .unwrap();
    let uri = format!("/api/instruments/{}/fundamentals", nova.id);
    let first = get(&state, &uri).await;
    let due = date(first["next_report"]["date"].as_str().unwrap());
    let later = state.with_clock(Clock::fixed(due + Duration::days(2)));
    let second = get(&later, &uri).await;
    assert_eq!(second["next_report"]["date"], first["next_report"]["date"]);
    assert_eq!(second["next_report"]["days_until"], -2);
    assert_eq!(
        second["next_report"]["reasons"][0]["code"],
        "estimate_passed"
    );
}
