//! Public API fixtures use only invented instruments and stored facts.
use super::*;
use crate::{
    db::{
        dividends,
        fundamental_values::{self, FetchWrite},
        fundamentals_sources::{
            Classification, GenerationWrite, LinkDefinition, LinkOrigin, LinkStatus,
        },
    },
    domain::{
        fundamentals::{
            AttemptStatus, FundamentalValue, FundamentalsSource, Metric, PeriodType, SplitEvent,
        },
        opportunity_evidence::dividend_check::DividendEvent,
    },
    providers::DividendCapture,
};
use chrono::{Datelike, Duration, Weekday};

pub(super) fn assert_unknown_reasons(value: &Value) {
    match value {
        Value::Object(object) => {
            if object.get("state").is_some_and(|state| state == "unknown") {
                let reasons = object["reasons"].as_array().expect("unknown reasons");
                assert!(!reasons.is_empty(), "{value}");
                assert!(
                    reasons
                        .iter()
                        .all(|r| r["code"].as_str().is_some_and(|c| !c.is_empty())),
                    "{value}"
                );
            }
            for child in object.values() {
                assert_unknown_reasons(child);
            }
            for (key, child) in object {
                if key.ends_with("_percent") {
                    if let Some(number) = child.as_str() {
                        assert_eq!(
                            number.split('.').nth(1).map(str::len),
                            Some(2),
                            "{key}: {number}"
                        );
                    }
                }
            }
        }
        Value::Array(array) => {
            for child in array {
                assert_unknown_reasons(child);
            }
        }
        _ => {}
    }
}

async fn response(state: &AppState, window: &str) -> Value {
    let revision = state.revision.current().await.unwrap();
    let body = get(state, window).await;
    assert_eq!(body["data_revision"], revision);
    assert_eq!(state.revision.current().await.unwrap(), revision);
    assert_unknown_reasons(&body);
    body
}

async fn history(state: &AppState, symbol: &str, end: &str) -> i64 {
    let id = seed(state, symbol, "SEK", "100", &[]).await;
    let today = state.clock.today();
    let mut closes = Vec::new();
    for offset in 0..=365 {
        let day = today - Duration::days(365 - offset);
        if matches!(day.weekday(), Weekday::Sat | Weekday::Sun) {
            continue;
        }
        let close = if day == today {
            end
        } else if day == today - Duration::days(7) {
            "100"
        } else {
            ["100", "103", "97", "101", "99"][offset as usize % 5]
        };
        closes.push((day.to_string(), close.to_owned()));
    }
    let refs: Vec<_> = closes
        .iter()
        .map(|(d, c)| (d.as_str(), c.as_str()))
        .collect();
    add_prices(state, id, MarketDataProvider::Yahoo, "SEK", &refs).await;
    id
}

struct Company {
    latest: &'static str,
    seen: &'static str,
    checked: &'static str,
    loss: bool,
    shrinking: bool,
    inconsistent: bool,
    splits: Vec<SplitEvent>,
}
impl Default for Company {
    fn default() -> Self {
        Self {
            latest: "2026-06-30",
            seen: "2026-08-01",
            checked: "2026-10-15",
            loss: false,
            shrinking: false,
            inconsistent: false,
            splits: vec![],
        }
    }
}
async fn company(state: &AppState, id: i64, spec: Company) {
    let mut values = Vec::new();
    let ends = if spec.latest == "2026-03-31" {
        [
            "2025-03-31",
            "2025-06-30",
            "2025-09-30",
            "2025-12-31",
            "2026-03-31",
        ]
    } else {
        [
            "2025-06-30",
            "2025-09-30",
            "2025-12-31",
            "2026-03-31",
            "2026-06-30",
        ]
    };
    for end in ends {
        for (metric, value) in [
            (
                Metric::Revenue,
                if spec.shrinking && end == spec.latest {
                    dec!(900)
                } else {
                    dec!(1000)
                },
            ),
            (
                Metric::DilutedEps,
                if spec.loss {
                    dec!(-1)
                } else if spec.inconsistent {
                    dec!(3)
                } else {
                    dec!(1)
                },
            ),
            (
                Metric::DilutedNetIncome,
                if spec.loss { dec!(-100) } else { dec!(100) },
            ),
            (Metric::DilutedShares, dec!(100)),
        ] {
            values.push(FundamentalValue {
                period_type: PeriodType::Quarter,
                period_end: date(end),
                metric,
                value,
                currency: Some("SEK".into()),
            });
        }
    }
    let mut tx = state.pool.begin().await.unwrap();
    let result = fundamental_values::write_fetch(
        &mut tx,
        &FetchWrite {
            instrument_id: id,
            expected: None,
            verified: None,
            discovery: Some(LinkDefinition {
                source: Some(FundamentalsSource::Yahoo),
                symbol: Some(format!("TEST-{id}")),
                security_type: Some("EQUITY".into()),
                classification: Classification::Equity,
                origin: LinkOrigin::Discovered,
                status: LinkStatus::Usable,
                candidates: "[]".into(),
            }),
            values,
            splits: spec.splits,
            at: format!("{}T12:00:00Z", spec.checked),
            seen_on: date(spec.seen),
            previous_success_on: None,
            status: AttemptStatus::Succeeded,
            reason: None,
        },
    )
    .await
    .unwrap();
    assert!(matches!(result, GenerationWrite::Applied(_)));
    tx.commit().await.unwrap();
}

fn check<'a>(body: &'a Value, symbol: &str, name: &str) -> &'a Value {
    &row(body, symbol)["evidence"]["checks"][name]
}
fn has_reason(check: &Value, code: &str) -> bool {
    check["reasons"]
        .as_array()
        .unwrap()
        .iter()
        .any(|r| r["code"] == code)
}

#[tokio::test]
async fn held_fixture_reaches_five_verdicts_with_loss_caution_and_company_free_fund() {
    let state = state().await;
    for (symbol, spec) in [
        ("CANDIDATE", Company::default()),
        (
            "POSSIBLE",
            Company {
                checked: "2026-10-10",
                ..Company::default()
            },
        ),
        (
            "LOSS",
            Company {
                loss: true,
                ..Company::default()
            },
        ),
        (
            "REASON",
            Company {
                shrinking: true,
                ..Company::default()
            },
        ),
    ] {
        let id = history(&state, symbol, "70").await;
        company(&state, id, spec).await;
    }
    seed(
        &state,
        "INSUFFICIENT",
        "SEK",
        "100",
        &[("2026-10-08", "100"), ("2026-10-15", "90")],
    )
    .await;
    history(&state, "NORMAL", "99.9").await;
    let fund = history(&state, "FUND", "70").await;
    crate::db::fundamentals_sources::correct(
        &state.pool,
        fund,
        Some(LinkDefinition {
            classification: Classification::Fund,
            status: LinkStatus::NotApplicable,
            security_type: Some("ETF".into()),
            ..LinkDefinition::pending()
        }),
        None,
        "2026-10-15T12:00:00Z",
    )
    .await
    .unwrap();
    seed(
        &state,
        "RISER",
        "SEK",
        "100",
        &[("2026-10-08", "100"), ("2026-10-15", "110")],
    )
    .await;
    seed(&state, "UNAVAILABLE", "SEK", "100", &[]).await;
    crate::db::testing::set_query_only(&state.pool).await;
    let body = response(&state, "7d").await;
    for (symbol, verdict) in [
        ("CANDIDATE", "sentiment_dip_candidate"),
        ("POSSIBLE", "possible_candidate_report_timing_unknown"),
        ("LOSS", "sentiment_dip_candidate"),
        ("REASON", "fell_for_a_reason"),
        ("INSUFFICIENT", "not_enough_data"),
        ("NORMAL", "normal_movement"),
        ("FUND", "sentiment_dip_candidate"),
    ] {
        assert_eq!(
            row(&body, symbol)["evidence"]["verdict"]["code"],
            verdict,
            "{symbol}: {}",
            row(&body, symbol)
        );
    }
    assert_eq!(
        row(&body, "LOSS")["evidence"]["verdict"]["cautions"],
        json!(["loss_making"])
    );
    assert_eq!(
        check(&body, "LOSS", "business")["profitable"]["value"],
        "-10.00"
    );
    assert_eq!(
        check(&body, "CANDIDATE", "business")["eps_consistency"]["gap_percent"],
        "0.00"
    );
    for name in ["business", "report_in_period", "report_coming_soon"] {
        assert_eq!(check(&body, "FUND", name)["state"], "not_applicable");
    }
    assert!(row(&body, "RISER")["evidence"].is_null());
    assert!(row(&body, "UNAVAILABLE")["evidence"].is_null());
    let last = &check(&body, "CANDIDATE", "report_in_period")["last_report"];
    assert_eq!(
        last,
        &json!({"kind":"upper_bound", "period_end":"2026-06-30", "first_seen":"2026-08-01"})
    );
    assert_eq!(
        check(&body, "POSSIBLE", "report_in_period")["state"],
        "unknown"
    );
    assert_eq!(
        row(&body, "INSUFFICIENT")["evidence"]["verdict"]["missing"],
        json!(["unusual_drop", "off_high", "report_in_period", "business"])
    );
}

#[tokio::test]
async fn unavailable_evidence_explains_short_gapped_split_stale_and_inconsistent_inputs() {
    let state = state().await;
    seed(
        &state,
        "SHORT",
        "SEK",
        "100",
        &[("2026-10-08", "100"), ("2026-10-15", "90")],
    )
    .await;
    let gap = history(&state, "GAP", "70").await;
    sqlx::query(
        "DELETE FROM prices WHERE instrument_id=? AND date BETWEEN '2026-05-01' AND '2026-05-20'",
    )
    .bind(gap)
    .execute(&state.pool)
    .await
    .unwrap();
    let split = history(&state, "SPLIT", "70").await;
    company(
        &state,
        split,
        Company {
            splits: vec![SplitEvent {
                date: date("2026-08-15"),
                numerator: dec!(2),
                denominator: dec!(1),
            }],
            ..Company::default()
        },
    )
    .await;
    sqlx::query("UPDATE prices SET fetched_at='2026-08-01T12:00:00Z' WHERE instrument_id=? AND date<'2026-08-01'").bind(split).execute(&state.pool).await.unwrap();
    let stale = history(&state, "STALE", "70").await;
    company(
        &state,
        stale,
        Company {
            latest: "2026-03-31",
            ..Company::default()
        },
    )
    .await;
    let eps = history(&state, "EPS", "70").await;
    company(
        &state,
        eps,
        Company {
            inconsistent: true,
            ..Company::default()
        },
    )
    .await;
    let body = response(&state, "7d").await;
    for (symbol, name, reason) in [
        ("SHORT", "unusual_drop", "price_history_too_short"),
        ("GAP", "unusual_drop", "price_history_gap"),
        ("SPLIT", "off_high", "split_in_lookback"),
        ("SHORT", "business", "no_fundamentals_source"),
        ("STALE", "business", "newer_report_expected"),
        ("EPS", "business", "eps_inconsistent"),
    ] {
        let check = check(&body, symbol, name);
        assert_eq!(check["state"], "unknown", "{symbol} {check}");
        assert!(has_reason(check, reason), "{symbol} {check}");
    }
    assert_eq!(
        check(&body, "SHORT", "unusual_drop")["reasons"][0]["required"],
        120
    );
    for symbol in ["SHORT", "GAP", "SPLIT"] {
        assert_eq!(
            check(&body, symbol, "unusual_drop")["sample_moves"],
            Value::Null,
            "{symbol} has no computed move sample"
        );
    }
    assert!(check(&body, "GAP", "unusual_drop")["reasons"]
        .as_array()
        .unwrap()
        .iter()
        .any(|r| r["start"].is_string()));
    assert_eq!(
        check(&body, "EPS", "business")["eps_consistency"]["state"],
        "inconsistent"
    );
    assert_eq!(
        check(&body, "EPS", "business")["eps_consistency"]["gap_percent"],
        "199.00"
    );
}

#[tokio::test]
async fn market_reference_needs_five_other_available_held_rows_and_excludes_itself() {
    let state = state().await;
    seed(
        &state,
        "SELF",
        "SEK",
        "100",
        &[("2026-10-08", "100"), ("2026-10-15", "50")],
    )
    .await;
    for (symbol, close) in [("A", "90"), ("B", "95"), ("C", "110"), ("D", "120")] {
        seed(
            &state,
            symbol,
            "SEK",
            "100",
            &[("2026-10-08", "100"), ("2026-10-15", close)],
        )
        .await;
    }
    seed(&state, "MISSING", "SEK", "100", &[]).await;
    let watch = seed(
        &state,
        "WATCH",
        "SEK",
        "100",
        &[("2026-10-08", "100"), ("2026-10-15", "1")],
    )
    .await;
    trade(&state, watch, "Sell", "2026-09-01", 10, "100", "SEK").await;
    let body = response(&state, "7d").await;
    let market = check(&body, "SELF", "market");
    assert_eq!(market["state"], "unknown");
    assert_eq!(market["count"], 4);
    assert!(has_reason(market, "insufficient_market_rows"));
    seed(
        &state,
        "E",
        "SEK",
        "100",
        &[("2026-10-08", "100"), ("2026-10-15", "130")],
    )
    .await;
    let body = response(&state, "7d").await;
    let market = check(&body, "SELF", "market");
    assert_eq!(market["count"], 5);
    assert_eq!(market["source"], "other_holdings_median");
    assert_eq!(market["reference_percent"], "10.00");
    assert_eq!(market["difference_percent"], "-60.00");
    assert_eq!(market["state"], "fell_more_than_market");
}

#[tokio::test]
async fn dividend_add_back_changes_only_the_unusual_drop_comparison() {
    let state = state().await;
    let id = history(&state, "DIVIDEND", "90").await;
    let mut tx = state.pool.begin().await.unwrap();
    dividends::capture(
        &mut tx,
        id,
        MarketDataProvider::Yahoo,
        date("2026-10-08")..=date("2026-10-15"),
        Some(date("2026-10-15")),
        &DividendCapture::Valid(vec![
            DividendEvent {
                ex_date: date("2026-10-08"),
                amount: dec!(100),
                currency: "SEK".into(),
            },
            DividendEvent {
                ex_date: date("2026-10-12"),
                amount: dec!(10),
                currency: "SEK".into(),
            },
            DividendEvent {
                ex_date: date("2026-10-15"),
                amount: dec!(0.5),
                currency: "SEK".into(),
            },
        ]),
    )
    .await
    .unwrap();
    tx.commit().await.unwrap();
    let body = response(&state, "7d").await;
    assert_eq!(
        row(&body, "DIVIDEND")["price_change"]["change_percent"],
        "-10.00"
    );
    let dividend = check(&body, "DIVIDEND", "ex_dividend");
    assert_eq!(dividend["state"], "yes");
    assert_eq!(dividend["events"].as_array().unwrap().len(), 2);
    assert_eq!(dividend["percent_of_start_close"], "10.50");
    let unusual = check(&body, "DIVIDEND", "unusual_drop");
    assert_eq!(unusual["change_percent"], "-10.00");
    assert_eq!(unusual["adjusted_change_percent"], "0.50");
    assert_eq!(unusual["state"], "no");
}

#[tokio::test]
async fn evidence_load_failure_fails_the_request_instead_of_claiming_unknown() {
    let state = state().await;
    seed(
        &state,
        "FAIL",
        "SEK",
        "100",
        &[("2026-10-08", "100"), ("2026-10-15", "90")],
    )
    .await;
    sqlx::query("DROP TABLE dividend_coverage")
        .execute(&state.pool)
        .await
        .unwrap();
    let (status, body) = send(&state, "GET", "/api/opportunities", Value::Null).await;
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(body["error"]["code"], "internal_error");
    sqlx::query("UPDATE prices SET close='110' WHERE date='2026-10-15'")
        .execute(&state.pool)
        .await
        .unwrap();
    let body = response(&state, "7d").await;
    assert!(
        row(&body, "FAIL")["evidence"].is_null(),
        "rising rows do not load evidence"
    );
}

#[tokio::test]
async fn report_history_explicitly_enables_relaxed_absence_with_an_old_figures_check() {
    let state = state().await;
    let id = history(&state, "RELAXED", "70").await;
    company(
        &state,
        id,
        Company {
            checked: "2026-10-10",
            ..Company::default()
        },
    )
    .await;
    sqlx::query("UPDATE fundamental_period_observations SET first_seen_on='2025-11-10', absent_fetch_on='2025-11-08' WHERE instrument_id=? AND period_end='2025-09-30'")
        .bind(id).execute(&state.pool).await.unwrap();
    let body = response(&state, "7d").await;
    let report = check(&body, "RELAXED", "report_in_period");
    assert_eq!(report["state"], "no");
    assert!(has_reason(report, "report_history_horizon"));
    assert_eq!(report["no_report_expected_before"], "2026-11-08");
    assert_eq!(
        row(&body, "RELAXED")["evidence"]["verdict"]["code"],
        "sentiment_dip_candidate"
    );
}

#[tokio::test]
async fn report_intervals_stay_intervals_and_first_fetch_is_only_an_upper_bound() {
    let state = state().await;
    for (symbol, absent, expected) in [
        ("INSIDE", Some("2026-10-02"), "yes"),
        ("OVERLAP", Some("2026-09-28"), "unknown"),
        ("BOUND", None, "unknown"),
    ] {
        let id = history(&state, symbol, "70").await;
        company(
            &state,
            id,
            Company {
                seen: "2026-10-05",
                ..Company::default()
            },
        )
        .await;
        sqlx::query("UPDATE fundamental_period_observations SET absent_fetch_on=? WHERE instrument_id=? AND period_end='2026-06-30'")
            .bind(absent).bind(id).execute(&state.pool).await.unwrap();
        let body = response(&state, "7d").await;
        let report = check(&body, symbol, "report_in_period");
        assert_eq!(report["state"], expected);
        let last = &report["last_report"];
        assert_eq!(
            last["kind"],
            if absent.is_some() {
                "interval"
            } else {
                "upper_bound"
            }
        );
        assert_eq!(last["first_seen"], "2026-10-05");
        assert!(last.get("date").is_none());
        assert_eq!(last["absent_on"], absent.map_or(Value::Null, |d| json!(d)));
    }
}

#[tokio::test]
async fn lookback_uses_source_capability_ledger_splits_and_120_closes_without_full_year_coverage() {
    let state = state().await;
    let id = history(&state, "RECENT_LISTING", "70").await;
    sqlx::query("DELETE FROM prices WHERE instrument_id=? AND date<'2026-04-15'")
        .bind(id)
        .execute(&state.pool)
        .await
        .unwrap();
    let split = history(&state, "LEDGER_SPLIT", "70").await;
    let (status, body) = send(
        &state,
        "POST",
        "/api/transactions",
        json!({"instrument_id":split,"type":"Split","trade_date":"2026-08-15","quantity":10}),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    // Even newly fetched Nasdaq Nordic history has no verified split adjustment.
    sqlx::query("UPDATE prices SET provider='NASDAQ_NORDIC' WHERE instrument_id=?")
        .bind(split)
        .execute(&state.pool)
        .await
        .unwrap();
    add_nasdaq_prices(
        &state,
        split,
        &[("2026-10-08", "100"), ("2026-10-15", "70")],
    )
    .await;
    let body = response(&state, "7d").await;
    assert_eq!(
        check(&body, "RECENT_LISTING", "unusual_drop")["state"],
        "yes"
    );
    assert!(has_reason(
        check(&body, "RECENT_LISTING", "off_high"),
        "price_history_too_short"
    ));
    assert!(has_reason(
        check(&body, "LEDGER_SPLIT", "unusual_drop"),
        "split_in_lookback"
    ));
    assert_eq!(
        row(&body, "LEDGER_SPLIT")["price_change"]["status"],
        "available"
    );
    // The same split is usable once all selected Yahoo closes are fetched after it.
    sqlx::query("UPDATE prices SET provider='YAHOO' WHERE instrument_id=?")
        .bind(split)
        .execute(&state.pool)
        .await
        .unwrap();
    let body = response(&state, "7d").await;
    assert_eq!(check(&body, "LEDGER_SPLIT", "unusual_drop")["state"], "yes");
}

#[tokio::test]
async fn a_tiny_fall_still_has_evidence_and_constant_history_has_an_unknown_reason() {
    let state = state().await;
    let id = history(&state, "CONSTANT", "99.99999999").await;
    sqlx::query("UPDATE prices SET close='100' WHERE instrument_id=? AND date<'2026-10-15'")
        .bind(id)
        .execute(&state.pool)
        .await
        .unwrap();
    let body = response(&state, "7d").await;
    assert_eq!(
        row(&body, "CONSTANT")["price_change"]["change_percent"],
        "0.00"
    );
    assert!(row(&body, "CONSTANT")["evidence"].is_object());
    let unusual = check(&body, "CONSTANT", "unusual_drop");
    assert_eq!(unusual["state"], "unknown");
    assert!(has_reason(unusual, "no_price_variation"));
}

#[tokio::test]
async fn confirmed_report_today_and_candidate_cautions_preserve_report_certainty() {
    let state = state().await;
    for (symbol, checked, resolved) in [
        ("CONFIRMED", "2026-10-15", true),
        ("CAUTIONS", "2026-10-15", false),
        ("POSSIBLE_CAUTIONS", "2026-10-10", false),
    ] {
        let id = history(&state, symbol, "70").await;
        company(
            &state,
            id,
            Company {
                seen: if resolved { "2026-10-15" } else { "2026-08-01" },
                loss: !resolved,
                checked,
                ..Company::default()
            },
        )
        .await;
        if !resolved {
            set_conviction(&state, id, "Other").await;
        }
        sqlx::query("INSERT INTO report_announcements (instrument_id, source, provider_symbol, scheduled_date, kind, fiscal_quarter_ending, first_seen_on, last_seen_on, state, resolved_period_end) VALUES (?, 'NASDAQ_US', ?, ?, 'company_announced', ?, '2026-10-01', '2026-10-15', ?, ?)")
            .bind(id).bind(format!("TEST-{id}"))
            .bind(if resolved { "2026-10-15" } else { "2026-10-18" })
            .bind(if resolved { "2026-06-30" } else { "2026-09-30" })
            .bind(if resolved { "resolved" } else { "active" })
            .bind(resolved.then_some("2026-06-30"))
            .execute(&state.pool).await.unwrap();
        // Evidence must never parse metadata belonging only to the stock source panel.
        sqlx::query("UPDATE instrument_fundamentals_sources SET candidates='invalid-panel-json' WHERE instrument_id=?")
            .bind(id).execute(&state.pool).await.unwrap();
    }
    let body = response(&state, "7d").await;
    assert_eq!(
        row(&body, "CONFIRMED")["evidence"]["verdict"]["code"],
        "fell_for_a_reason"
    );
    assert_eq!(
        check(&body, "CONFIRMED", "report_in_period")["last_report"],
        json!({"kind":"confirmed", "date":"2026-10-15"})
    );
    for (symbol, expected) in [
        ("CAUTIONS", "sentiment_dip_candidate"),
        (
            "POSSIBLE_CAUTIONS",
            "possible_candidate_report_timing_unknown",
        ),
    ] {
        let verdict = &row(&body, symbol)["evidence"]["verdict"];
        assert_eq!(verdict["code"], expected);
        assert_eq!(
            verdict["cautions"],
            json!(["loss_making", "report_coming_soon", "no_target"])
        );
        let soon = check(&body, symbol, "report_coming_soon");
        assert_eq!(soon["state"], "yes");
        assert_eq!(soon["report"]["certainty"], "confirmed");
        assert_eq!(soon["report"]["basis"], "company_announced");
        assert_eq!(soon["report"]["date"], "2026-10-18");
        assert_eq!(soon["report"]["days_until"], 3);
    }
}

#[tokio::test]
async fn missing_fx_holding_has_available_native_change_but_unknown_room_to_buy() {
    let state = state().await;
    seed(
        &state,
        "MISSING-FX",
        "USD",
        "100",
        &[("2026-10-08", "100"), ("2026-10-15", "90")],
    )
    .await;
    crate::db::testing::set_query_only(&state.pool).await;
    let body = response(&state, "7d").await;
    let holding = row(&body, "MISSING-FX");
    assert_eq!(holding["price_change"]["status"], "available");
    assert_eq!(holding["price_change"]["change_percent"], "-10.00");
    let room = check(&body, "MISSING-FX", "room_to_buy");
    assert_eq!(room["state"], "unknown", "{room}");
    assert!(has_reason(room, "valuation_unavailable"), "{room}");
}

async fn index_closes(state: &AppState, closes: &[(&str, &str)]) {
    for (day, close) in closes {
        crate::db::market_indexes::upsert(
            &state.pool,
            &crate::db::market_indexes::NewIndexClose {
                index_code: "OMXSPI".into(),
                provider: MarketDataProvider::Yahoo,
                provider_symbol: "^OMXSPI".into(),
                currency: "SEK".into(),
                date: date(day),
                close: close.parse().unwrap(),
                fetched_at: "2026-10-15T12:00:00Z".into(),
            },
        )
        .await
        .unwrap();
    }
}

#[tokio::test]
async fn market_sources_and_fallback_have_exact_contract_and_never_change_the_verdict() {
    let state = state().await;
    let id = history(&state, "CONTEXT", "72").await;
    company(
        &state,
        id,
        Company {
            loss: true,
            ..Company::default()
        },
    )
    .await;
    set_conviction(&state, id, "Other").await;
    for symbol in ["PEER1", "PEER2", "PEER3", "PEER4", "PEER5"] {
        seed(
            &state,
            symbol,
            "SEK",
            "100",
            &[("2026-10-08", "100"), ("2026-10-15", "99")],
        )
        .await;
    }
    let median = response(&state, "7d").await;
    let expected_median = json!({"state":"fell_more_than_market", "reasons":[],
        "source":"other_holdings_median", "change_percent":"-28.00", "reference_percent":"-1.00",
        "difference_percent":"-27.00", "margin_percent":"3.00", "count":5});
    assert_eq!(check(&median, "CONTEXT", "market"), &expected_median);
    let verdict = row(&median, "CONTEXT")["evidence"]["verdict"].clone();
    assert!(!verdict["deciding"].as_array().unwrap().is_empty());
    assert!(verdict["cautions"]
        .as_array()
        .unwrap()
        .contains(&json!("loss_making")));
    // Change only the synthetic listing's market; its stored closes remain identical.
    sqlx::query(
        "UPDATE instrument_provider_symbols SET provider_symbol='CONTEXT.ST' WHERE instrument_id=?",
    )
    .bind(id)
    .execute(&state.pool)
    .await
    .unwrap();
    index_closes(&state, &[("2026-10-07", "100"), ("2026-10-14", "60")]).await;
    let indexed = response(&state, "7d").await;
    assert_eq!(
        check(&indexed, "CONTEXT", "market"),
        &json!({
            "state":"fell_less_than_market", "reasons":[], "source":"market_index",
            "index_code":"OMXSPI", "index_name":"OMX Stockholm all-share",
            "index_start_date":"2026-10-07", "index_end_date":"2026-10-14",
            "change_percent":"-28.00", "reference_percent":"-40.00", "difference_percent":"12.00",
            "margin_percent":"3.00", "count":5
        })
    );
    assert_eq!(row(&indexed, "CONTEXT")["evidence"]["verdict"], verdict);
    // A stale end is not silently paired to the fresh holding close.
    sqlx::query("DELETE FROM market_index_closes WHERE date='2026-10-14'")
        .execute(&state.pool)
        .await
        .unwrap();
    let fallback = response(&state, "7d").await;
    let mut expected_fallback = expected_median;
    expected_fallback["reasons"] =
        json!([{"code":"market_index_unavailable", "detail":"OMX Stockholm all-share"}]);
    assert_eq!(check(&fallback, "CONTEXT", "market"), &expected_fallback);
    assert_eq!(row(&fallback, "CONTEXT")["evidence"]["verdict"], verdict);
    sqlx::query("DELETE FROM market_index_closes")
        .execute(&state.pool)
        .await
        .unwrap();
    assert_eq!(
        check(&response(&state, "7d").await, "CONTEXT", "market"),
        &expected_fallback
    );
}

#[tokio::test]
async fn evidence_reuses_listing_sources_for_certificate_classification() {
    let state = state().await;
    let id = history(&state, "CERTIFICATE", "72").await;
    company(&state, id, Company::default()).await;
    sqlx::query("UPDATE instrument_provider_symbols SET asset_class='TRACKER_CERTIFICATES', provider_symbol='CERTIFICATE.ST' WHERE instrument_id=?")
        .bind(id)
        .execute(&state.pool)
        .await
        .unwrap();
    index_closes(&state, &[("2026-10-08", "100"), ("2026-10-15", "99")]).await;
    crate::db::testing::set_query_only(&state.pool).await;
    let body = response(&state, "7d").await;
    assert_eq!(
        check(&body, "CERTIFICATE", "market")["source"],
        "market_index"
    );
    for name in ["business", "report_in_period", "report_coming_soon"] {
        assert_eq!(check(&body, "CERTIFICATE", name)["state"], "not_applicable");
    }
}

#[tokio::test]
async fn market_index_load_or_decode_failure_fails_the_endpoint_instead_of_falling_back() {
    for corrupt in [false, true] {
        let state = state().await;
        let id = seed(
            &state,
            "INDEXED",
            "SEK",
            "100",
            &[("2026-10-08", "100"), ("2026-10-15", "90")],
        )
        .await;
        sqlx::query("UPDATE instrument_provider_symbols SET provider_symbol='INDEXED.ST' WHERE instrument_id=?")
            .bind(id).execute(&state.pool).await.unwrap();
        if corrupt {
            index_closes(&state, &[("2026-10-08", "100")]).await;
            sqlx::query("UPDATE market_index_closes SET close='invalid'")
                .execute(&state.pool)
                .await
                .unwrap();
        } else {
            sqlx::query("DROP TABLE market_index_closes")
                .execute(&state.pool)
                .await
                .unwrap();
        }
        let (status, body) = send(&state, "GET", "/api/opportunities?window=7d", Value::Null).await;
        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
        assert_eq!(body["error"]["code"], "internal_error");
    }
}
