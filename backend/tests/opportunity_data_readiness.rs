// Examples are not run as tests by cargo test; include the actual measurement code here.
#[path = "../examples/opportunity_data_readiness/mod.rs"]
mod readiness;

use axum::{body::Body, http::Request};
use ticker_tape_tally_board_backend::{
    api,
    clock::Clock,
    db, demo,
    market_data::MarketDataService,
    providers::{FakeFxRateProvider, FakePriceProvider},
    state::AppState,
};
use tower::ServiceExt;

#[tokio::test]
async fn demo_aggregate_counts_match_opportunities_and_do_not_disclose_instruments() {
    let clock = Clock::fixed("2026-11-11".parse().unwrap());
    let pool = db::memory_pool().await.unwrap();
    demo::seed(&pool, clock.today()).await.unwrap();
    let summary = readiness::measure(&pool, &clock).await.unwrap();
    let output = summary.render();
    for id in 1..=9 {
        assert!(output.contains(&format!("R{id} ")));
    }
    for instrument in db::instruments::list(&pool).await.unwrap() {
        assert!(!output.contains(&instrument.symbol));
        assert!(!output.contains(&instrument.name));
        if let Some(isin) = instrument.isin {
            assert!(!output.contains(&isin));
        }
    }
    let state = AppState::with_market_data(
        pool.clone(),
        MarketDataService::with_providers(FakePriceProvider::new(), FakeFxRateProvider::new()),
    )
    .with_clock(clock);
    for window in ["7d", "14d", "30d"] {
        let response = api::router(state.clone())
            .oneshot(
                Request::builder()
                    .uri(format!("/api/opportunities?window={window}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert!(response.status().is_success());
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        let rows = body["rows"].as_array().unwrap();
        let available = rows
            .iter()
            .filter(|row| row["price_change"]["status"] == "available")
            .count();
        let table = &summary.tables[&format!("R8 {window} change")];
        assert_eq!(table.population, rows.len());
        assert_eq!(table.counts["available"], available);
        assert_eq!(
            table.counts["median_reference_ready"],
            if available >= 6 { available } else { 0 }
        );
        for rule in ["strict", "relaxed"] {
            let table = &summary.tables[&format!("R3 {window} {rule}")];
            assert_eq!(
                table.counts["yes"] + table.counts["no"] + table.counts["unknown"],
                table.population
            );
            assert_eq!(
                table.counts["answered"],
                table.counts["yes"] + table.counts["no"]
            );
        }
    }
    pool.close().await;
}

#[tokio::test]
async fn empty_ledger_prints_all_measurements_with_suppressed_shares() {
    let pool = db::memory_pool().await.unwrap();
    let summary = readiness::measure(&pool, &Clock::fixed("2026-11-11".parse().unwrap()))
        .await
        .unwrap();
    let output = summary.render();
    for id in 1..=9 {
        assert!(output.contains(&format!("R{id} ")));
    }
    assert!(!output.contains('%'));
    assert!(output.contains("share=suppressed"));
    pool.close().await;
}

#[tokio::test]
async fn thirty_day_answered_shares_include_raw_and_excluded_first_fetch_populations() {
    let clock = Clock::fixed("2026-11-11".parse().unwrap());
    let pool = db::memory_pool().await.unwrap();
    demo::seed(&pool, clock.today()).await.unwrap();
    sqlx::query("DELETE FROM report_announcements")
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("UPDATE fundamental_period_observations SET first_seen_on='2026-11-05', absent_fetch_on=NULL")
        .execute(&pool).await.unwrap();
    let summary = readiness::measure(&pool, &clock).await.unwrap();
    for rule in ["strict", "relaxed"] {
        let raw = &summary.tables[&format!("R3 30d {rule}")];
        let reasons = &summary.tables[&format!("R3 30d {rule} unknown_reasons")];
        let adjusted = &summary.tables[&format!("R3 30d {rule} excluding_first_fetch")];
        let excluded = reasons.counts["first_fetch_without_baseline"];
        assert!(excluded > 0);
        assert_eq!(adjusted.population, raw.population - excluded);
        assert_eq!(adjusted.counts["answered"], raw.counts["answered"]);
        assert!(adjusted.population < 5);
        let output = summary.render();
        let section = output
            .split(&format!("R3 30d {rule} excluding_first_fetch:"))
            .nth(1)
            .unwrap()
            .split("\n\n")
            .next()
            .unwrap();
        assert!(section.contains("share=suppressed"));
    }
    pool.close().await;
}

#[tokio::test]
async fn malformed_rows_are_counted_without_disclosure_and_other_evaluations_continue() {
    let clock = Clock::fixed("2026-11-11".parse().unwrap());
    for (mutation, operation) in [
        ("UPDATE instrument_fundamentals_sources SET last_success_at='PRIVATE_BAD_TIMESTAMP' WHERE instrument_id=(SELECT MIN(instrument_id) FROM instrument_fundamentals_sources)", "R1-R5 company inputs"),
        ("UPDATE prices SET fetched_at='PRIVATE_BAD_TIMESTAMP' WHERE id=(SELECT MIN(id) FROM prices)", "R9 close finality"),
        ("UPDATE prices SET date='PRIVATE_BAD_DATE' WHERE id=(SELECT MIN(id) FROM prices)", "R9 close finality"),
        ("UPDATE transactions SET type='SELL', quantity=-999999 WHERE id=(SELECT MIN(id) FROM transactions)", "R1-R8 held position"),
    ] {
        let pool = db::memory_pool().await.unwrap();
        demo::seed(&pool, clock.today()).await.unwrap();
        sqlx::query(mutation).execute(&pool).await.unwrap();
        let summary = readiness::measure(&pool, &clock).await.unwrap();
        assert_eq!(summary.failures[operation], 1, "{operation}");
        let output = summary.render();
        assert!(output.contains("INCOMPLETE; evaluation failed"));
        assert!(!output.contains("PRIVATE_BAD"));
        assert!(summary.tables["R8 30d change"].counts["available"] > 0);
        assert!(summary.tables["R9 stored_close_finality"].counts["final_close"] > 0);
        if operation == "R9 close finality" {
            assert_eq!(summary.tables["R9 stored_close_finality"].counts["evaluation_failed"], 1);
        }
        pool.close().await;
    }
}

#[tokio::test]
async fn whole_run_failure_names_only_the_failed_operation() {
    let pool = db::memory_pool().await.unwrap();
    pool.close().await;
    let error = readiness::measure(&pool, &Clock::fixed("2026-11-11".parse().unwrap()))
        .await
        .err()
        .unwrap();
    assert_eq!(error, "Measurement failed: R1 instrument inventory.");
}

#[tokio::test]
async fn first_fetch_compares_against_annual_as_well_as_quarterly_observations() {
    let clock = Clock::fixed("2026-11-11".parse().unwrap());
    let pool = db::memory_pool().await.unwrap();
    demo::seed(&pool, clock.today()).await.unwrap();
    sqlx::query("DELETE FROM report_announcements")
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("UPDATE fundamental_period_observations SET first_seen_on='2026-11-05', absent_fetch_on=NULL")
        .execute(&pool).await.unwrap();
    let before = readiness::measure(&pool, &clock).await.unwrap();
    let affected =
        before.tables["R3 30d strict unknown_reasons"].counts["first_fetch_without_baseline"];
    assert!(affected > 0);
    // Fixture annual observations precede the first quarter observation. Retain the
    // instrument's earlier observation baseline instead of calling this a first fetch.
    sqlx::query("INSERT INTO fundamental_period_observations (instrument_id, period_type, period_end, provider_symbol, first_seen_on) SELECT DISTINCT instrument_id, 'ANNUAL', '2024-12-31', 'SYNTHETIC', '2026-10-01' FROM fundamental_period_observations WHERE period_type='QUARTER'")
        .execute(&pool).await.unwrap();
    let after = readiness::measure(&pool, &clock).await.unwrap();
    assert_eq!(
        after.tables["R3 30d strict unknown_reasons"]
            .counts
            .get("first_fetch_without_baseline")
            .copied()
            .unwrap_or(0),
        0
    );
    assert_eq!(
        after.tables["R3 30d strict unknown_reasons"].counts["figures_without_absence_baseline"],
        affected
    );
    assert_eq!(
        after.tables["R3 30d strict excluding_first_fetch"].population,
        after.tables["R3 30d strict"].population
    );
    pool.close().await;
}
