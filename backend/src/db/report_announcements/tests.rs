use super::*;
use crate::{
    db::{
        fundamentals_sources::{self, LinkDefinition},
        instruments, provider_symbols,
    },
    providers::MarketDataProvider,
};
fn d(s: &str) -> NaiveDate {
    s.parse().unwrap()
}
async fn instrument(pool: &SqlitePool) -> i64 {
    instruments::upsert(
        pool,
        &instruments::NewInstrument {
            symbol: "TEST".into(),
            exchange: "NASDAQ".into(),
            name: "Test".into(),
            kind: "STOCK".into(),
            currency: "USD".into(),
            isin: None,
        },
    )
    .await
    .unwrap()
    .0
    .id
}
async fn mapping(pool: &SqlitePool, id: i64, symbol: &str, enabled: bool) {
    provider_symbols::upsert(
        pool,
        &provider_symbols::NewProviderSymbol {
            instrument_id: id,
            provider: MarketDataProvider::Yahoo,
            provider_symbol: symbol.into(),
            asset_class: None,
            currency: Some("USD".into()),
            enabled,
            created_at: "2026-11-09T12:00:00Z".into(),
            updated_at: "2026-11-09T12:00:00Z".into(),
        },
    )
    .await
    .unwrap();
}
#[tokio::test]
async fn moves_retain_rows_and_record_the_replacement_and_freeze_resolved_kind() {
    let pool = crate::db::testing::memory_pool().await;
    let id = instrument(&pool).await;
    mapping(&pool, id, "TEST", true).await;
    let ticker = ticker(&pool, id).await.unwrap().unwrap();
    let today = d("2026-11-09");
    let old = AnnouncementDate {
        date: d("2026-12-02"),
        kind: AnnouncementKind::Expected,
        fiscal_quarter: None,
        session: None,
    };
    let mut tx = pool.begin().await.unwrap();
    apply(
        &mut tx,
        id,
        &ticker,
        today,
        &[AnnouncementChange::Insert(old.clone())],
    )
    .await
    .unwrap();
    let mut moved = old.clone();
    moved.date = d("2026-12-04");
    moved.fiscal_quarter = Some(d("2026-10-31"));
    moved.kind = AnnouncementKind::CompanyAnnounced;
    moved.session = Some(ReportSession::BeforeMarketOpen);
    let entries = list(&mut *tx, id).await.unwrap();
    let plan = reconcile_date(&entries, &moved, false, today, None);
    apply(&mut tx, id, &ticker, today, &plan).await.unwrap();
    let entries = list(&mut *tx, id).await.unwrap();
    assert_eq!(entries.len(), 2);
    assert_eq!(entries[0].state, AnnouncementState::Superseded);
    assert_eq!(entries[0].superseded_by, Some(entries[1].id));
    let fallback = ReportDate {
        certainty: ReportCertainty::Unavailable,
        basis: None,
        date: None,
        days_until: None,
        period_end: None,
        observed_between: None,
        reasons: vec![],
    };
    let next = scheduled_next(today, fallback, &entries);
    assert_eq!(next.date, Some(d("2026-12-04")));
    assert_eq!(next.certainty, ReportCertainty::Confirmed);
    apply(
        &mut tx,
        id,
        &ticker,
        today,
        &[AnnouncementChange::Resolve {
            id: entries[1].id,
            period_end: d("2026-10-31"),
        }],
    )
    .await
    .unwrap();
    let entries = list(&mut *tx, id).await.unwrap();
    moved.kind = AnnouncementKind::Expected;
    assert!(reconcile_date(&entries, &moved, false, today, None).is_empty());
    tx.commit().await.unwrap();
    let entries = list(&pool, id).await.unwrap();
    assert_eq!(entries[1].report.kind, AnnouncementKind::CompanyAnnounced);
    let mut tx = pool.begin().await.unwrap();
    provider_symbols::delete_by_instrument_id_in_tx(&mut tx, id)
        .await
        .unwrap();
    instruments::delete_in_tx(&mut tx, id).await.unwrap();
    tx.commit().await.unwrap();
    assert!(list(&pool, id).await.unwrap().is_empty());
}

#[tokio::test]
async fn failed_scan_preserves_last_success_and_still_consumes_the_daily_attempt() {
    use crate::domain::fundamentals::AttemptStatus;
    let pool = crate::db::testing::memory_pool().await;
    let day = d("2026-12-02");
    let mut tx = pool.begin().await.unwrap();
    record_scan(
        &mut tx,
        day,
        d("2026-12-02"),
        "2026-12-02T12:00:00Z",
        AttemptStatus::Succeeded,
    )
    .await
    .unwrap();
    record_scan(
        &mut tx,
        day,
        d("2026-12-03"),
        "2026-12-03T12:00:00Z",
        AttemptStatus::Failed,
    )
    .await
    .unwrap();
    tx.commit().await.unwrap();
    let row = sqlx::query("SELECT * FROM report_calendar_scans")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(row.get::<String, _>("scanned_at"), "2026-12-03T12:00:00Z");
    assert_eq!(
        row.get::<String, _>("outcome"),
        AttemptStatus::Failed.as_str()
    );
    assert_eq!(
        row.get::<String, _>("last_success_at"),
        "2026-12-02T12:00:00Z"
    );
    assert_eq!(row.get::<String, _>("last_success_on"), "2026-12-02");
    assert!(scanned_today(&pool, d("2026-12-03"))
        .await
        .unwrap()
        .contains(&day));
    assert!(!scanned_today(&pool, d("2026-12-04"))
        .await
        .unwrap()
        .contains(&day));
}

#[tokio::test]
async fn observations_choose_one_first_sighting_across_same_company_symbols() {
    let pool = crate::db::testing::memory_pool().await;
    let id = instrument(&pool).await;
    for (symbol, first, absent) in [
        ("OLD", "2026-11-02", None),
        ("RIGHT", "2026-11-03", Some("2026-11-01")),
    ] {
        sqlx::query("INSERT INTO fundamental_period_observations (instrument_id, period_type, period_end, provider_symbol, first_seen_on, absent_fetch_on) VALUES (?, ?, ?, ?, ?, ?)")
            .bind(id).bind(PeriodType::Quarter.as_str()).bind("2026-09-30").bind(symbol).bind(first).bind(absent).execute(&pool).await.unwrap();
    }
    let observed = observations(&pool, id).await.unwrap();
    assert_eq!(observed.len(), 1);
    assert_eq!(observed[0].first_seen, d("2026-11-02"));
    assert_eq!(observed[0].absent_on, None);
    assert!(real_report_history(&[], &observed).is_empty());
}
#[tokio::test]
async fn ticker_rechecks_enabled_mapping_and_figures_generation() {
    let pool = crate::db::testing::memory_pool().await;
    let id = instrument(&pool).await;
    mapping(&pool, id, "GUESS", false).await;
    assert!(ticker(&pool, id).await.unwrap().is_none());
    let definition = LinkDefinition {
        symbol: Some("FIGURES".into()),
        ..LinkDefinition::pending()
    };
    let first = fundamentals_sources::correct(
        &pool,
        id,
        Some(definition.clone()),
        None,
        "2026-11-09T12:00:00Z",
    )
    .await
    .unwrap();
    assert_eq!(
        ticker(&pool, id).await.unwrap(),
        Some(UsTicker {
            symbol: "FIGURES".into(),
            link_generation: Some(first.token.generation)
        })
    );
    let second =
        fundamentals_sources::correct(&pool, id, Some(definition), None, "2026-11-09T12:00:01Z")
            .await
            .unwrap();
    assert_ne!(first.token.generation, second.token.generation);
    assert_eq!(
        ticker(&pool, id).await.unwrap().unwrap().link_generation,
        Some(second.token.generation)
    );
    mapping(&pool, id, "PRICE", true).await;
    assert_eq!(
        ticker(&pool, id).await.unwrap(),
        Some(UsTicker {
            symbol: "PRICE".into(),
            link_generation: None
        })
    );
}
#[tokio::test]
async fn unknown_stored_identity_kind_state_and_lookup_codes_are_decode_errors() {
    let pool = crate::db::testing::memory_pool().await;
    let id = instrument(&pool).await;
    mapping(&pool, id, "TEST", true).await;
    let ticker = ticker(&pool, id).await.unwrap().unwrap();
    let mut tx = pool.begin().await.unwrap();
    apply(
        &mut tx,
        id,
        &ticker,
        d("2026-11-09"),
        &[AnnouncementChange::Insert(AnnouncementDate {
            date: d("2026-12-02"),
            kind: AnnouncementKind::Expected,
            fiscal_quarter: None,
            session: None,
        })],
    )
    .await
    .unwrap();
    tx.commit().await.unwrap();
    for (sql, valid) in [
        ("UPDATE report_announcements SET source=?", "NASDAQ_US"),
        ("UPDATE report_announcements SET kind=?", "expected"),
        ("UPDATE report_announcements SET state=?", "active"),
    ] {
        sqlx::query(sql)
            .bind("unknown")
            .execute(&pool)
            .await
            .unwrap();
        assert!(matches!(list(&pool, id).await, Err(RepoError::Decode(_))));
        sqlx::query(sql).bind(valid).execute(&pool).await.unwrap();
    }
    let mut tx = pool.begin().await.unwrap();
    record_lookup(&mut tx, id, &ticker, d("2026-11-09"), LookupOutcome::Failed)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    sqlx::query("UPDATE report_date_lookups SET last_outcome='unknown'")
        .execute(&pool)
        .await
        .unwrap();
    assert!(matches!(
        lookup(&pool, id, &ticker).await,
        Err(RepoError::Decode(_))
    ));
}
