//! Stored index closes, independent of instruments and their price status.
use crate::{db::RepoError, providers::MarketDataProvider};
use chrono::NaiveDate;
use rust_decimal::Decimal;
use sqlx::SqlitePool;

#[derive(Clone, Debug, sqlx::FromRow)]
pub struct IndexCloseRow {
    pub index_code: String,
    pub provider: String,
    pub date: String,
    pub provider_symbol: String,
    pub close: String,
    pub currency: String,
    pub fetched_at: String,
}

impl IndexCloseRow {
    pub fn date_value(&self) -> Result<NaiveDate, RepoError> {
        self.date
            .parse()
            .map_err(|error| RepoError::Decode(format!("index {} date: {error}", self.index_code)))
    }
    pub fn close_decimal(&self) -> Result<Decimal, RepoError> {
        self.close
            .parse()
            .map_err(|error| RepoError::Decode(format!("index {} close: {error}", self.index_code)))
    }
}

pub struct NewIndexClose {
    pub index_code: String,
    pub provider: MarketDataProvider,
    pub date: NaiveDate,
    pub provider_symbol: String,
    pub close: Decimal,
    pub currency: String,
    pub fetched_at: String,
}

pub async fn upsert<'e, E>(executor: E, row: &NewIndexClose) -> Result<(), RepoError>
where
    E: sqlx::Executor<'e, Database = sqlx::Sqlite>,
{
    sqlx::query("INSERT INTO market_index_closes (index_code, provider, date, provider_symbol, close, currency, fetched_at) VALUES (?, ?, ?, ?, ?, ?, ?) ON CONFLICT (index_code, provider, date) DO UPDATE SET provider_symbol=excluded.provider_symbol, close=excluded.close, currency=excluded.currency, fetched_at=excluded.fetched_at")
        .bind(&row.index_code).bind(row.provider.as_str()).bind(row.date.to_string())
        .bind(&row.provider_symbol).bind(row.close.to_string()).bind(&row.currency).bind(&row.fetched_at)
        .execute(executor).await?;
    Ok(())
}

/// Inclusive, ordered range for the configured symbol; callers can include their pairing tolerance in `from`.
pub async fn list_in_range(
    pool: &SqlitePool,
    index_code: &str,
    provider: MarketDataProvider,
    provider_symbol: &str,
    from: NaiveDate,
    to: NaiveDate,
) -> Result<Vec<IndexCloseRow>, RepoError> {
    Ok(sqlx::query_as("SELECT index_code, provider, date, provider_symbol, close, currency, fetched_at FROM market_index_closes WHERE index_code=? AND provider=? AND provider_symbol=? AND date>=? AND date<=? ORDER BY date")
        .bind(index_code).bind(provider.as_str()).bind(provider_symbol).bind(from.to_string()).bind(to.to_string())
        .fetch_all(pool).await?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    #[tokio::test]
    async fn migration_is_additive_and_keeps_prior_checksums_and_new_sql_lf() {
        let pool = SqlitePool::connect("sqlite::memory:").await.unwrap();
        let migrations = sqlx::migrate!("./migrations");
        let previous = sqlx::migrate::Migrator {
            migrations: migrations
                .iter()
                .filter(|migration| migration.version < 14)
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
        assert_eq!(before.last().unwrap().0, 13);
        migrations.run(&pool).await.unwrap();
        let after: Vec<(i64, Vec<u8>)> = sqlx::query_as(
            "SELECT version, checksum FROM _sqlx_migrations WHERE version < 14 ORDER BY version",
        )
        .fetch_all(&pool)
        .await
        .unwrap();
        assert_eq!(before, after);
        assert!(!include_bytes!("../../migrations/0014_market_index_closes.sql").contains(&b'\r'));
        assert!(crate::db::instruments::list(&pool)
            .await
            .unwrap()
            .is_empty());
    }

    #[tokio::test]
    async fn index_provider_date_key_overwrites_and_range_is_inclusive_and_ordered() {
        let pool = crate::db::testing::memory_pool().await;
        let mut row = NewIndexClose {
            index_code: "SP500".into(),
            provider: MarketDataProvider::Yahoo,
            date: "2026-11-12".parse().unwrap(),
            provider_symbol: "^GSPC".into(),
            close: dec!(100),
            currency: "USD".into(),
            fetched_at: "2026-11-12T14:00:00Z".into(),
        };
        upsert(&pool, &row).await.unwrap();
        row.close = dec!(101);
        row.fetched_at = "2026-11-12T22:00:00Z".into();
        upsert(&pool, &row).await.unwrap();
        row.provider = MarketDataProvider::Manual;
        row.close = dec!(999);
        upsert(&pool, &row).await.unwrap();
        row.provider = MarketDataProvider::Yahoo;
        row.date = "2026-11-11".parse().unwrap();
        upsert(&pool, &row).await.unwrap();
        let rows = list_in_range(
            &pool,
            "SP500",
            MarketDataProvider::Yahoo,
            &row.provider_symbol,
            row.date,
            row.date.succ_opt().unwrap(),
        )
        .await
        .unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].date_value().unwrap(), row.date);
        assert_eq!(rows[1].close_decimal().unwrap(), dec!(101));
        assert_eq!(rows[1].fetched_at, "2026-11-12T22:00:00Z");
        assert!(list_in_range(
            &pool,
            "AEX",
            MarketDataProvider::Yahoo,
            "^AEX",
            row.date,
            row.date
        )
        .await
        .unwrap()
        .is_empty());
        assert_eq!(
            list_in_range(
                &pool,
                "SP500",
                MarketDataProvider::Manual,
                &row.provider_symbol,
                row.date.succ_opt().unwrap(),
                row.date.succ_opt().unwrap()
            )
            .await
            .unwrap()[0]
                .close_decimal()
                .unwrap(),
            dec!(999)
        );
    }
    #[tokio::test]
    async fn closes_from_a_previous_provider_symbol_are_not_returned() {
        let pool = crate::db::testing::memory_pool().await;
        let mut row = NewIndexClose {
            index_code: "OMXSPI".into(),
            provider: MarketDataProvider::Yahoo,
            date: "2026-11-11".parse().unwrap(),
            provider_symbol: "^OMXSPI".into(),
            close: dec!(100),
            currency: "SEK".into(),
            fetched_at: "2026-11-13T12:00:00Z".into(),
        };
        upsert(&pool, &row).await.unwrap();
        row.provider_symbol = "^OMX".into();
        row.date = row.date.succ_opt().unwrap();
        upsert(&pool, &row).await.unwrap();
        let rows = list_in_range(
            &pool,
            &row.index_code,
            row.provider,
            &row.provider_symbol,
            row.date.pred_opt().unwrap(),
            row.date,
        )
        .await
        .unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].provider_symbol, "^OMX");
        assert_eq!(rows[0].date_value().unwrap(), row.date);
    }
}
