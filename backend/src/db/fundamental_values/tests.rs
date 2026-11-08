use super::*;
use crate::{
    db::{
        self,
        fundamentals_sources::{Classification, CorrectionError, LinkStatus, PreviousLink},
        instruments,
    },
    domain::fundamentals::LinkOrigin,
};
use rust_decimal_macros::dec;

async fn setup() -> (SqlitePool, i64) {
    let pool = db::testing::memory_pool().await;
    let (instrument, _) = instruments::upsert(
        &pool,
        &instruments::NewInstrument {
            symbol: "TEST".into(),
            exchange: "NYSE".into(),
            name: "Test".into(),
            kind: "STOCK".into(),
            currency: "SEK".into(),
            isin: None,
        },
    )
    .await
    .unwrap();
    (pool, instrument.id)
}
fn definition(symbol: &str) -> LinkDefinition {
    LinkDefinition {
        source: Some(FundamentalsSource::Yahoo),
        symbol: Some(symbol.into()),
        security_type: Some("EQUITY".into()),
        classification: Classification::Equity,
        origin: LinkOrigin::Discovered,
        status: LinkStatus::Usable,
        candidates: "[]".into(),
    }
}
fn value(end: &str, amount: Decimal) -> FundamentalValue {
    FundamentalValue {
        period_type: PeriodType::Quarter,
        period_end: end.parse().unwrap(),
        metric: Metric::Revenue,
        value: amount,
        currency: Some("EUR".into()),
    }
}
async fn write(
    pool: &SqlitePool,
    id: i64,
    expected: Option<LinkToken>,
    discovery: Option<LinkDefinition>,
    at: &str,
    values: Vec<FundamentalValue>,
) -> GenerationWrite<SourceLink> {
    let clock = crate::clock::Clock::fixed(at[..10].parse().unwrap());
    let previous_success_on = fundamentals_sources::find(pool, id)
        .await
        .unwrap()
        .and_then(|link| link.last_success_at)
        .map(|at| clock.local_date(chrono::DateTime::parse_from_rfc3339(&at).unwrap().to_utc()));
    let mut tx = pool.begin_with("BEGIN IMMEDIATE").await.unwrap();
    let result = write_fetch(
        &mut tx,
        &FetchWrite {
            instrument_id: id,
            expected,
            discovery,
            verified: None,
            values,
            splits: vec![SplitEvent {
                date: "2026-09-01".parse().unwrap(),
                numerator: dec!(2),
                denominator: dec!(1),
            }],
            at: at.into(),
            seen_on: clock.today(),
            previous_success_on,
            status: AttemptStatus::Succeeded,
            reason: None,
        },
    )
    .await
    .unwrap();
    tx.commit().await.unwrap();
    result
}
#[tokio::test]
async fn revisions_retain_first_seen_and_latest_active_symbol_wins() {
    let (pool, id) = setup().await;
    write(
        &pool,
        id,
        None,
        Some(definition("OLD")),
        "2026-10-01T12:00:00Z",
        vec![value("2026-06-30", dec!(10))],
    )
    .await;
    let link = fundamentals_sources::find(&pool, id)
        .await
        .unwrap()
        .unwrap();
    write(
        &pool,
        id,
        Some(link.token),
        None,
        "2026-10-02T12:00:00Z",
        vec![value("2026-06-30", dec!(11)), value("2026-09-30", dec!(12))],
    )
    .await;
    let values = active_values(&pool, id).await.unwrap();
    let revised = values
        .iter()
        .find(|v| v.input.period_end.to_string() == "2026-06-30")
        .unwrap();
    assert_eq!(revised.input.value, dec!(11));
    assert_eq!(revised.first_seen_at, "2026-10-01T12:00:00Z");
    let observed = active_observations(&pool, id).await.unwrap();
    assert_eq!(observed[0].absent_fetch_on, None);
    assert_eq!(
        observed[1].absent_fetch_on,
        Some("2026-10-01".parse().unwrap())
    );
    let link = fundamentals_sources::correct(
        &pool,
        id,
        Some(definition("NEW")),
        Some(PreviousLink::SameCompany),
        "2026-10-03T12:00:00Z",
    )
    .await
    .unwrap();
    assert!(link.last_success_at.is_none());
    write(
        &pool,
        id,
        Some(link.token),
        None,
        "2026-10-04T12:00:00Z",
        vec![value("2026-06-30", dec!(13))],
    )
    .await;
    assert_eq!(
        active_values(&pool, id)
            .await
            .unwrap()
            .iter()
            .find(|v| v.input.period_end.to_string() == "2026-06-30")
            .unwrap()
            .input
            .value,
        dec!(13)
    );
    let count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM fundamental_values WHERE instrument_id=?")
            .bind(id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(count, 3);
    assert_eq!(active_observations(&pool, id).await.unwrap().len(), 2);
}

#[tokio::test]
async fn report_estimate_history_uses_only_narrow_active_first_observations() {
    let (pool, id) = setup().await;
    let GenerationWrite::Applied(link) = write(
        &pool,
        id,
        None,
        Some(definition("TEST")),
        "2026-10-01T12:00:00Z",
        vec![value("2026-06-30", dec!(10))],
    )
    .await
    else {
        panic!()
    };
    let GenerationWrite::Applied(link) = write(
        &pool,
        id,
        Some(link.token),
        None,
        "2026-10-05T12:00:00Z",
        vec![value("2026-09-30", dec!(10))],
    )
    .await
    else {
        panic!()
    };
    assert_eq!(
        active_report_history(&pool, id).await.unwrap(),
        vec![("2026-09-30".parse().unwrap(), "2026-10-03".parse().unwrap())]
    );
    write(
        &pool,
        id,
        Some(link.token),
        None,
        "2027-01-31T12:00:00Z",
        vec![value("2026-12-31", dec!(10))],
    )
    .await;
    assert_eq!(active_report_history(&pool, id).await.unwrap().len(), 1);
    fundamentals_sources::correct(
        &pool,
        id,
        Some(definition("RIGHT")),
        Some(PreviousLink::WrongCompany),
        "2027-02-01T12:00:00Z",
    )
    .await
    .unwrap();
    assert!(active_report_history(&pool, id).await.unwrap().is_empty());
}
#[tokio::test]
async fn wrong_company_correction_is_atomic_and_all_active_reads_exclude_old_rows() {
    let (pool, id) = setup().await;
    write(
        &pool,
        id,
        None,
        Some(definition("TEST")),
        "2026-10-01T12:00:00Z",
        vec![value("2026-09-30", dec!(10))],
    )
    .await;
    let before = fundamentals_sources::find(&pool, id)
        .await
        .unwrap()
        .unwrap();
    let revision_before = crate::data_revision::current(&pool).await.unwrap();
    assert_eq!(active_splits(&pool, id).await.unwrap().len(), 1);
    assert!(matches!(
        fundamentals_sources::correct(&pool, id, None, None, "2026-10-02T12:00:00Z").await,
        Err(CorrectionError::PreviousLinkRequired)
    ));
    assert_eq!(
        fundamentals_sources::find(&pool, id)
            .await
            .unwrap()
            .unwrap()
            .token,
        before.token
    );
    let corrected = fundamentals_sources::correct(
        &pool,
        id,
        Some(definition("TEST")),
        Some(PreviousLink::WrongCompany),
        "2026-10-02T12:00:00Z",
    )
    .await
    .unwrap();
    assert_eq!(
        crate::data_revision::current(&pool).await.unwrap(),
        revision_before
    );
    assert!(active_values(&pool, id).await.unwrap().is_empty());
    assert!(active_observations(&pool, id).await.unwrap().is_empty());
    assert!(report_currencies(&pool, id).await.unwrap().is_empty());
    assert!(active_splits(&pool, id).await.unwrap().is_empty());
    assert!(corrected.last_attempt_at.is_none());
    assert!(corrected.last_success_at.is_none());
    assert_eq!(corrected.consecutive_empty, 0);
    assert_eq!(
        write(
            &pool,
            id,
            Some(before.token),
            None,
            "2026-10-03T12:00:00Z",
            vec![value("2026-09-30", dec!(99))]
        )
        .await,
        GenerationWrite::Superseded
    );
    write(
        &pool,
        id,
        Some(corrected.token),
        None,
        "2026-10-03T12:00:00Z",
        vec![value("2026-09-30", dec!(20))],
    )
    .await;
    assert_eq!(
        active_values(&pool, id).await.unwrap()[0].input.value,
        dec!(20)
    );
    assert_eq!(
        active_observations(&pool, id).await.unwrap()[0].absent_fetch_on,
        None
    );
    let history: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM fundamental_values WHERE set_aside_reason='wrong company'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(history, 1);
}
#[tokio::test]
async fn manual_link_survives_discovery_and_symbol_mismatch_is_superseded() {
    let (pool, id) = setup().await;
    let link = fundamentals_sources::correct(
        &pool,
        id,
        Some(definition("MANUAL")),
        None,
        "2026-10-01T12:00:00Z",
    )
    .await
    .unwrap();
    assert_eq!(
        write(
            &pool,
            id,
            Some(link.token.clone()),
            Some(definition("AUTO")),
            "2026-10-02T12:00:00Z",
            vec![]
        )
        .await,
        GenerationWrite::Superseded
    );
    let mut token = link.token.clone();
    token.symbol = Some("OTHER".into());
    assert_eq!(
        write(&pool, id, Some(token), None, "2026-10-02T12:00:00Z", vec![]).await,
        GenerationWrite::Superseded
    );
    assert_eq!(
        fundamentals_sources::find(&pool, id)
            .await
            .unwrap()
            .unwrap()
            .token,
        link.token
    );
    sqlx::query("UPDATE instrument_fundamentals_sources SET classification='FUTURE_CODE' WHERE instrument_id=?").bind(id).execute(&pool).await.unwrap();
    assert!(matches!(
        fundamentals_sources::find(&pool, id).await,
        Err(RepoError::Decode(_))
    ));
}
