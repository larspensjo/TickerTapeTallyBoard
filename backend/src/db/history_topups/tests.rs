use super::*;
use crate::{
    db::{instruments, provider_symbols, testing},
    providers::MarketDataProvider,
};

fn day() -> NaiveDate {
    "2026-11-11".parse().unwrap()
}

async fn mapping(pool: &SqlitePool) -> i64 {
    let (instrument, _) = instruments::upsert(
        pool,
        &instruments::NewInstrument {
            symbol: "TEST".into(),
            exchange: "TEST".into(),
            name: "Test".into(),
            kind: "STOCK".into(),
            currency: "SEK".into(),
            isin: None,
        },
    )
    .await
    .unwrap();
    provider_symbols::upsert(
        pool,
        &provider_symbols::NewProviderSymbol {
            instrument_id: instrument.id,
            provider: MarketDataProvider::Yahoo,
            provider_symbol: "TEST.ST".into(),
            asset_class: None,
            currency: Some("SEK".into()),
            enabled: true,
            created_at: "test".into(),
            updated_at: "test".into(),
        },
    )
    .await
    .unwrap()
    .id
}

#[tokio::test]
async fn reservation_is_daily_and_failed_attempt_does_not_erase_success() {
    let pool = testing::memory_pool().await;
    let id = mapping(&pool).await;
    let mut tx = pool.begin().await.unwrap();
    assert!(begin(&mut tx, id, day(), day(), day()).await.unwrap());
    assert!(!begin(&mut tx, id, day(), day(), day()).await.unwrap());
    finish(&mut tx, id, day(), Some(day()), Some("{\"needs\":[]}"))
        .await
        .unwrap();
    tx.commit().await.unwrap();
    assert!(attempted_on(&pool, id, day()).await.unwrap());
    let tomorrow = day().succ_opt().unwrap();
    let mut tx = pool.begin().await.unwrap();
    assert!(begin(&mut tx, id, tomorrow, day(), tomorrow).await.unwrap());
    finish(&mut tx, id, tomorrow, None, None).await.unwrap();
    tx.commit().await.unwrap();
    assert_eq!(
        last_success(&pool, id)
            .await
            .unwrap()
            .unwrap()
            .remaining_needs
            .as_deref(),
        Some("{\"needs\":[]}")
    );
    provider_symbols::delete_by_instrument_id_in_tx(&mut pool.acquire().await.unwrap(), 1)
        .await
        .unwrap();
    assert!(!attempted_on(&pool, id, day()).await.unwrap());
}

#[tokio::test]
async fn migration_upgrades_previous_schema_and_preserves_checksums() {
    let pool = SqlitePool::connect("sqlite::memory:").await.unwrap();
    let migrations = sqlx::migrate!("./migrations");
    let previous = sqlx::migrate::Migrator {
        migrations: migrations
            .iter()
            .filter(|migration| migration.version < 12)
            .cloned()
            .collect::<Vec<_>>()
            .into(),
        ..sqlx::migrate::Migrator::DEFAULT
    };
    previous.run(&pool).await.unwrap();
    let before: Vec<(i64, Vec<u8>)> =
        sqlx::query_as("SELECT version, checksum FROM _sqlx_migrations ORDER BY version")
            .fetch_all(&pool)
            .await
            .unwrap();
    assert_eq!(before.last().unwrap().0, 11);
    migrations.run(&pool).await.unwrap();
    let after: Vec<(i64, Vec<u8>)> = sqlx::query_as(
        "SELECT version, checksum FROM _sqlx_migrations WHERE version < 12 ORDER BY version",
    )
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(before, after);
    assert!(!include_bytes!("../../../migrations/0012_record_history_topups.sql").contains(&b'\r'));
    let id = mapping(&pool).await;
    let mut conn = pool.acquire().await.unwrap();
    assert!(begin(&mut conn, id, day(), day(), day()).await.unwrap());
}
