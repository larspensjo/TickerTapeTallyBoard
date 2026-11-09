use crate::{
    api::test_support::{instrument, send},
    clock::Clock,
    config::Mode,
    db::{
        self,
        fundamental_values::{self, FetchWrite},
        fundamentals_sources::{self, *},
    },
    domain::fundamentals::{FundamentalValue, Metric, PeriodType},
    market_data::{MarketDataService, ProviderRegistry},
    providers::{
        yahoo_fundamentals::ListingDetails, FakeFundamentalsProvider, FakeFxRateProvider,
        FakePriceProvider, FakeSymbolSearchProvider, MarketDataProvider, SymbolSearchMatch,
    },
    state::AppState,
};
use axum::http::StatusCode;
use rust_decimal_macros::dec;
use serde_json::{json, Value};

fn day() -> chrono::NaiveDate {
    "2026-11-08".parse().unwrap()
}
fn values(sales: i64) -> Vec<FundamentalValue> {
    ["2025-12-31", "2026-03-31", "2026-06-30", "2026-09-30"]
        .into_iter()
        .flat_map(|end| {
            [
                FundamentalValue {
                    period_type: PeriodType::Quarter,
                    period_end: end.parse().unwrap(),
                    metric: Metric::Revenue,
                    value: sales.into(),
                    currency: Some("SEK".into()),
                },
                FundamentalValue {
                    period_type: PeriodType::Quarter,
                    period_end: end.parse().unwrap(),
                    metric: Metric::CommonNetIncome,
                    value: dec!(10),
                    currency: Some("SEK".into()),
                },
            ]
        })
        .collect()
}
async fn setup() -> (
    AppState,
    i64,
    FakeFundamentalsProvider,
    FakeSymbolSearchProvider,
) {
    let figures = FakeFundamentalsProvider::new();
    let search = FakeSymbolSearchProvider::with_provider(MarketDataProvider::Yahoo);
    let state = AppState::with_market_data(
        db::memory_pool().await.unwrap(),
        MarketDataService::with_provider_registry(
            ProviderRegistry::new()
                .with_price_provider(MarketDataProvider::Yahoo, FakePriceProvider::new())
                .with_fx_provider(FakeFxRateProvider::new())
                .with_fundamentals_provider(figures.clone())
                .with_symbol_search_provider(MarketDataProvider::Yahoo, search.clone()),
        ),
    )
    .with_clock(Clock::fixed(day()));
    let id = instrument(&state, "TEST", "STO", "SEK").await;
    send(
        &state,
        "PUT",
        &format!("/api/instruments/{id}/conviction"),
        json!({"conviction":"High"}),
    )
    .await;
    (state, id, figures, search)
}
fn uri(id: i64) -> String {
    format!("/api/instruments/{id}/fundamentals-source")
}
async fn seed(state: &AppState, id: i64) {
    let definition = LinkDefinition {
        source: Some(FundamentalsSource::Yahoo),
        symbol: Some("OLD".into()),
        security_type: Some("EQUITY".into()),
        classification: Classification::Equity,
        status: LinkStatus::Usable,
        ..LinkDefinition::pending()
    };
    let mut tx = state.pool.begin().await.unwrap();
    fundamental_values::write_fetch(
        &mut tx,
        &FetchWrite {
            instrument_id: id,
            expected: None,
            discovery: Some(definition),
            verified: None,
            values: values(100),
            splits: vec![],
            at: "2026-11-01T12:00:00Z".into(),
            seen_on: "2026-11-01".parse().unwrap(),
            previous_success_on: None,
            status: AttemptStatus::Succeeded,
            reason: None,
        },
    )
    .await
    .unwrap();
    tx.commit().await.unwrap();
    // A prior empty attempt exercises the full bookkeeping reset, including
    // the nonzero empty count and the observation baseline for a later fetch.
    let old = fundamentals_sources::find(&state.pool, id)
        .await
        .unwrap()
        .unwrap();
    let mut tx = state.pool.begin().await.unwrap();
    fundamental_values::write_fetch(
        &mut tx,
        &FetchWrite {
            instrument_id: id,
            expected: Some(old.token),
            discovery: None,
            verified: None,
            values: vec![],
            splits: vec![],
            at: "2026-11-02T12:00:00Z".into(),
            seen_on: "2026-11-02".parse().unwrap(),
            previous_success_on: Some("2026-11-01".parse().unwrap()),
            status: AttemptStatus::Empty,
            reason: Some("no_quarterly_data".into()),
        },
    )
    .await
    .unwrap();
    tx.commit().await.unwrap();
}
async fn panel(state: &AppState, id: i64) -> Value {
    let (status, body) = send(
        state,
        "GET",
        &format!("/api/instruments/{id}/fundamentals"),
        Value::Null,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    body
}

#[tokio::test]
async fn preview_put_and_delete_round_trip_are_read_only_or_publish_revision() {
    let (state, id, figures, search) = setup().await;
    search.push_response(Ok(vec![SymbolSearchMatch {
        provider: MarketDataProvider::Yahoo,
        provider_symbol: "NEW".into(),
        name: Some("New company".into()),
        quote_type: Some("EQUITY".into()),
        exchange: None,
        currency: None,
        asset_class: None,
    }]));
    figures.push_response(Ok(values(200)));
    let revision = state.revision.current().await.unwrap();
    let (status, body) = send(
        &state,
        "GET",
        &format!("{}/preview?symbol=NEW", uri(id)),
        Value::Null,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["status"], "found");
    assert_eq!(body["name"], "New company");
    assert_eq!(body["security_type"], "EQUITY");
    assert_eq!(body["report_currency"], "SEK");
    assert_eq!(body["quarters"], 4);
    assert_eq!(body["latest_period_end"], "2026-09-30");
    assert_eq!(body["data_revision"], revision);
    assert_eq!(figures.calls().len(), 1);
    assert!(figures.listing_calls().is_empty());
    assert!(fundamentals_sources::find(&state.pool, id)
        .await
        .unwrap()
        .is_none());
    assert_eq!(
        send(
            &state,
            "PUT",
            &uri(id),
            json!({"provider":"YAHOO","provider_symbol":"NEW"})
        )
        .await
        .0,
        StatusCode::NO_CONTENT
    );
    assert_ne!(revision, state.revision.current().await.unwrap());
    let p = panel(&state, id).await;
    assert_eq!(p["source"]["origin"], "MANUAL");
    assert_eq!(p["source"]["symbol"], "NEW");
    assert_eq!(
        send(&state, "DELETE", &uri(id), json!({})).await.0,
        StatusCode::NO_CONTENT
    );
    let link = fundamentals_sources::find(&state.pool, id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(link.token.generation, 2);
    assert_eq!(link.definition.origin, LinkOrigin::Discovered);
    assert_eq!(link.definition.status, LinkStatus::Pending);
    assert_eq!(
        send(
            &state,
            "PUT",
            &uri(id),
            json!({"provider":null,"not_a_company":true})
        )
        .await
        .0,
        StatusCode::NO_CONTENT
    );
    assert_eq!(panel(&state, id).await["pe"]["certainty"], "not_applicable");
}

#[tokio::test]
async fn corrections_require_choice_and_rejection_changes_nothing() {
    let (state, id, _, _) = setup().await;
    seed(&state, id).await;
    let before = fundamentals_sources::find(&state.pool, id).await.unwrap();
    let revision = state.revision.current().await.unwrap();
    for (method, body) in [
        ("PUT", json!({"provider":"YAHOO","provider_symbol":"NEW"})),
        ("DELETE", json!({})),
        ("PUT", json!({"provider":null,"not_a_company":true})),
    ] {
        let (status, body) = send(&state, method, &uri(id), body).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(body["error"]["code"], "previous_link_required");
        assert_eq!(
            fundamentals_sources::find(&state.pool, id).await.unwrap(),
            before
        );
        assert_eq!(
            fundamental_values::active_values(&state.pool, id)
                .await
                .unwrap()
                .len(),
            8
        );
        assert_eq!(
            fundamental_values::active_observations(&state.pool, id, None)
                .await
                .unwrap()
                .len(),
            4
        );
        assert_eq!(revision, state.revision.current().await.unwrap());
    }
}

#[tokio::test]
async fn wrong_company_sets_aside_atomically_same_company_retains_and_refresh_merges() {
    for previous in ["same_company", "wrong_company"] {
        let (state, id, figures, search) = setup().await;
        seed(&state, id).await;
        let before = fundamentals_sources::find(&state.pool, id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(before.consecutive_empty, 1);
        assert_eq!(
            send(
                &state,
                "PUT",
                &uri(id),
                json!({"provider":"YAHOO","provider_symbol":"NEW","previous_link":previous})
            )
            .await
            .0,
            StatusCode::NO_CONTENT
        );
        let link = fundamentals_sources::find(&state.pool, id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(link.token.generation, before.token.generation + 1);
        assert!(link.last_success_at.is_none());
        assert!(link.last_attempt_at.is_none());
        assert!(link.last_attempt_status.is_none());
        assert!(link.last_attempt_reason.is_none());
        assert_eq!(link.consecutive_empty, 0);
        let p = panel(&state, id).await;
        if previous == "wrong_company" {
            assert!(fundamental_values::has_set_aside_values(&state.pool, id)
                .await
                .unwrap());
            assert!(
                fundamental_values::active_observations(&state.pool, id, None)
                    .await
                    .unwrap()
                    .is_empty()
            );
            assert_eq!(p["source"]["awaiting_figures"], true);
            assert_eq!(p["pe"]["reasons"][0]["code"], "awaiting_figures");
            assert!(p["quarters"].as_array().unwrap().is_empty());
        } else {
            assert_eq!(
                p["profit_margin"]["value"]
                    .as_str()
                    .unwrap()
                    .parse::<rust_decimal::Decimal>()
                    .unwrap(),
                dec!(0.1)
            );
            assert_eq!(p["source"]["pending_update"], true);
        }
        let newer = values(200)
            .into_iter()
            .filter(|v| v.period_end != "2025-12-31".parse::<chrono::NaiveDate>().unwrap())
            .collect();
        figures.push_response(Ok(newer));
        figures.push_listing(Ok(ListingDetails {
            security_type: Some("EQUITY".into()),
            splits: vec![],
        }));
        let (status, result) = send(
            &state,
            "POST",
            "/api/prices/refresh",
            json!({"mode":"latest"}),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{result}");
        assert_eq!(result["fundamentals"]["fetched"], 1, "{result}");
        assert!(
            search.calls().is_empty(),
            "manual link must bypass discovery"
        );
        assert_eq!(figures.calls()[0].symbol, "NEW");
        let p = panel(&state, id).await;
        assert_eq!(p["source"]["symbol"], "NEW");
        assert_eq!(p["source"]["origin"], "MANUAL");
        assert_eq!(p["source"]["security_type"], "EQUITY");
        assert_eq!(p["source"]["pending_update"], false);
        assert_eq!(p["source"]["awaiting_figures"], false);
        assert_eq!(
            p["profit_margin"]["value"]
                .as_str()
                .unwrap()
                .parse::<rust_decimal::Decimal>()
                .unwrap(),
            if previous == "same_company" {
                dec!(40) / dec!(700)
            } else {
                dec!(0.05)
            }
        );
        let active = fundamental_values::active_values(&state.pool, id)
            .await
            .unwrap();
        assert_eq!(active.len(), if previous == "same_company" { 8 } else { 6 });
        for v in &active {
            assert_eq!(
                v.provider_symbol,
                if v.input.period_end == "2025-12-31".parse::<chrono::NaiveDate>().unwrap() {
                    "OLD"
                } else {
                    "NEW"
                }
            );
        }
        if previous == "same_company" {
            assert_eq!(p["quarters"][0]["sales"]["value"], "100");
        }
        let observations = fundamental_values::active_observations(&state.pool, id, None)
            .await
            .unwrap();
        assert_eq!(
            observations.len(),
            if previous == "same_company" { 4 } else { 3 }
        );
        if previous == "wrong_company" {
            assert!(observations
                .iter()
                .all(|o| o.provider_symbol == "NEW" && o.absent_fetch_on.is_none()));
        }
    }
}

#[tokio::test]
async fn validation_unknown_instrument_and_offline_demo() {
    let (state, id, figures, search) = setup().await;
    for (method, path, body, status, code) in [
        (
            "PUT",
            uri(id),
            json!({"provider":"BAD","provider_symbol":"NEW"}),
            400,
            "invalid_provider",
        ),
        (
            "PUT",
            uri(id),
            json!({"provider":"YAHOO","provider_symbol":"  "}),
            400,
            "invalid_provider_symbol",
        ),
        (
            "GET",
            format!("{}/preview?symbol=%20", uri(id)),
            Value::Null,
            400,
            "invalid_provider_symbol",
        ),
        (
            "PUT",
            uri(999),
            json!({"provider":"YAHOO","provider_symbol":"NEW"}),
            404,
            "not_found",
        ),
        ("DELETE", uri(999), json!({}), 404, "not_found"),
        (
            "GET",
            format!("{}/preview?symbol=NEW", uri(999)),
            Value::Null,
            404,
            "not_found",
        ),
    ] {
        let (actual, body) = send(&state, method, &path, body).await;
        assert_eq!(actual.as_u16(), status);
        assert_eq!(body["error"]["code"], code);
    }
    let state = state.with_mode(Mode::Demo);
    for method in ["PUT", "DELETE"] {
        assert_eq!(
            send(&state, method, &uri(id), json!({})).await.0,
            StatusCode::FORBIDDEN
        );
    }
    let (status, body) = send(
        &state,
        "GET",
        &format!("{}/preview?symbol=NEW", uri(id)),
        Value::Null,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["status"], "provider_unavailable");
    assert!(figures.calls().is_empty());
    assert!(search.calls().is_empty());
}

#[tokio::test]
async fn not_a_company_sets_aside_history_without_promising_a_future_fetch() {
    let (state, id, figures, _) = setup().await;
    seed(&state, id).await;
    assert_eq!(
        send(
            &state,
            "PUT",
            &uri(id),
            json!({
                "provider": null, "not_a_company": true, "previous_link": "wrong_company"
            })
        )
        .await
        .0,
        StatusCode::NO_CONTENT
    );
    let p = panel(&state, id).await;
    assert_eq!(p["pe"]["certainty"], "not_applicable");
    assert_eq!(p["source"]["has_active_figures"], false);
    assert_eq!(p["source"]["awaiting_figures"], false);
    assert_eq!(p["source"]["pending_update"], false);
    assert!(p["quarters"].as_array().unwrap().is_empty());
    assert!(fundamental_values::has_set_aside_values(&state.pool, id)
        .await
        .unwrap());
    send(
        &state,
        "POST",
        "/api/prices/refresh",
        json!({"mode":"latest"}),
    )
    .await;
    assert!(figures.calls().is_empty());
}

#[tokio::test]
async fn preview_no_match_and_provider_failures_are_read_only() {
    let (state, id, figures, search) = setup().await;
    let revision = state.revision.current().await.unwrap();
    search.push_response(Ok(vec![]));
    let path = format!("{}/preview?symbol=NEW", uri(id));
    assert_eq!(
        send(&state, "GET", &path, Value::Null).await.1["status"],
        "no_match"
    );
    assert!(figures.calls().is_empty());
    search.push_response(Err(crate::providers::ProviderError::provider_error(
        "YAHOO",
        "offline fake",
    )));
    assert_eq!(
        send(&state, "GET", &path, Value::Null).await.1["status"],
        "provider_unavailable"
    );
    search.push_response(Ok(vec![SymbolSearchMatch {
        provider: MarketDataProvider::Yahoo,
        provider_symbol: "NEW".into(),
        name: Some("New company".into()),
        quote_type: None,
        exchange: None,
        currency: None,
        asset_class: None,
    }]));
    figures.push_response(Err(crate::providers::ProviderError::provider_error(
        "YAHOO",
        "offline fake",
    )));
    assert_eq!(
        send(&state, "GET", &path, Value::Null).await.1["status"],
        "provider_unavailable"
    );
    assert_eq!(figures.calls().len(), 1);
    assert!(figures.listing_calls().is_empty());
    assert!(fundamentals_sources::find(&state.pool, id)
        .await
        .unwrap()
        .is_none());
    assert_eq!(revision, state.revision.current().await.unwrap());
}

#[tokio::test]
async fn automatic_uses_query_choice_without_a_delete_body_and_keeps_error_codes() {
    use axum::{
        body::{to_bytes, Body},
        http::Request,
    };
    use tower::ServiceExt;
    let (state, id, _, _) = setup().await;
    seed(&state, id).await;
    for (query, expected, code) in [
        ("", StatusCode::BAD_REQUEST, Some("previous_link_required")),
        (
            "?previous_link=invalid",
            StatusCode::BAD_REQUEST,
            Some("invalid_json"),
        ),
        ("?previous_link=wrong_company", StatusCode::NO_CONTENT, None),
    ] {
        let response = crate::api::router(state.clone())
            .oneshot(
                Request::builder()
                    .method("DELETE")
                    .uri(format!("{}{query}", uri(id)))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), expected);
        if let Some(code) = code {
            let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
            let body: Value = serde_json::from_slice(&bytes).unwrap();
            assert_eq!(body["error"]["code"], code);
        }
    }
    let p = panel(&state, id).await;
    assert_eq!(p["source"]["origin"], "DISCOVERED");
    assert_eq!(p["source"]["awaiting_figures"], true);
    assert_eq!(p["source"]["has_active_figures"], false);
}

#[tokio::test]
async fn awaiting_ends_on_completed_attempts_and_persists_through_outages() {
    for (status, link_status, classification, reason, awaiting, figure_reason) in [
        (
            AttemptStatus::Failed,
            LinkStatus::Pending,
            Classification::Unknown,
            "provider_unavailable",
            true,
            "awaiting_figures",
        ),
        (
            AttemptStatus::Failed,
            LinkStatus::Pending,
            Classification::Unknown,
            "rate_limited",
            true,
            "awaiting_figures",
        ),
        (
            AttemptStatus::Empty,
            LinkStatus::Unavailable,
            Classification::Equity,
            "no_quarterly_data",
            false,
            "too_few_quarters",
        ),
        (
            AttemptStatus::Empty,
            LinkStatus::NoMatch,
            Classification::Unknown,
            "no_match",
            false,
            "no_fundamentals_source",
        ),
        (
            AttemptStatus::Empty,
            LinkStatus::Ambiguous,
            Classification::Unknown,
            "ambiguous",
            false,
            "source_ambiguous",
        ),
        (
            AttemptStatus::Empty,
            LinkStatus::Unavailable,
            Classification::Unknown,
            "security_type_unknown",
            false,
            "security_type_unknown",
        ),
    ] {
        let (state, id, _, _) = setup().await;
        seed(&state, id).await;
        let link = fundamentals_sources::correct(
            &state.pool,
            id,
            None,
            Some(PreviousLink::WrongCompany),
            "2026-11-08T00:00:00Z",
        )
        .await
        .unwrap();
        assert_eq!(panel(&state, id).await["source"]["awaiting_figures"], true);
        let mut tx = state.pool.begin().await.unwrap();
        fundamental_values::write_fetch(
            &mut tx,
            &FetchWrite {
                instrument_id: id,
                expected: Some(link.token),
                discovery: Some(LinkDefinition {
                    status: link_status,
                    classification,
                    ..LinkDefinition::pending()
                }),
                verified: None,
                values: vec![],
                splits: vec![],
                at: "2026-11-08T12:00:00Z".into(),
                seen_on: day(),
                previous_success_on: None,
                status,
                reason: Some(reason.into()),
            },
        )
        .await
        .unwrap();
        tx.commit().await.unwrap();
        let p = panel(&state, id).await;
        assert_eq!(p["source"]["awaiting_figures"], awaiting, "{reason}");
        for metric in ["pe", "ps", "profit_margin", "sales_growth"] {
            assert_eq!(
                p[metric]["reasons"][0]["code"], figure_reason,
                "{reason}: {p}"
            );
        }
    }
}

#[tokio::test]
async fn source_dates_use_the_application_local_calendar_and_preserve_timestamps() {
    let (state, id, _, _) = setup().await;
    let instant = "2026-11-01T23:30:00Z"
        .parse::<chrono::DateTime<chrono::Utc>>()
        .unwrap();
    let state = state.with_clock(Clock::fixed_instant(instant));
    let empty = panel(&state, id).await;
    assert!(empty["source"]["last_success_on"].is_null());
    assert!(empty["source"]["last_attempt_on"].is_null());
    let mut tx = state.pool.begin().await.unwrap();
    fundamental_values::write_fetch(
        &mut tx,
        &FetchWrite {
            instrument_id: id,
            expected: None,
            discovery: Some(LinkDefinition::pending()),
            verified: None,
            values: vec![],
            splits: vec![],
            at: instant.to_rfc3339(),
            seen_on: state.clock.today(),
            previous_success_on: None,
            status: AttemptStatus::Empty,
            reason: None,
        },
    )
    .await
    .unwrap();
    tx.commit().await.unwrap();
    let p = panel(&state, id).await;
    let local_day = state.clock.local_date(instant).to_string();
    assert_eq!(p["source"]["last_success_on"], local_day);
    assert_eq!(p["source"]["last_attempt_on"], local_day);
    assert_eq!(p["source"]["last_success"], instant.to_rfc3339());
    assert_eq!(p["source"]["last_attempt_at"], instant.to_rfc3339());
}
