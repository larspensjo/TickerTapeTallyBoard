use sqlx::sqlite::SqlitePool;

use crate::db::RepoError;

const LIST_SQL: &str = "SELECT id, instrument_id, provider, provider_symbol, currency, enabled, created_at, updated_at \
    FROM instrument_provider_symbols ORDER BY instrument_id, provider";
const FIND_SQL: &str = "SELECT id, instrument_id, provider, provider_symbol, currency, enabled, created_at, updated_at \
    FROM instrument_provider_symbols WHERE id = ?";
const FIND_BY_INSTRUMENT_PROVIDER_SQL: &str = "SELECT id, instrument_id, provider, provider_symbol, currency, enabled, created_at, updated_at \
    FROM instrument_provider_symbols WHERE instrument_id = ? AND provider = ?";
const LIST_BY_PROVIDER_SYMBOL_SQL: &str = "SELECT id, instrument_id, provider, provider_symbol, currency, enabled, created_at, updated_at \
    FROM instrument_provider_symbols WHERE provider = ? AND provider_symbol = ? ORDER BY instrument_id, id";
const UPSERT_SQL: &str = "INSERT INTO instrument_provider_symbols \
       (instrument_id, provider, provider_symbol, currency, enabled, created_at, updated_at) \
     VALUES (?, ?, ?, ?, ?, ?, ?) \
     ON CONFLICT (instrument_id, provider) DO UPDATE SET \
       provider_symbol = excluded.provider_symbol, \
       currency = excluded.currency, \
       enabled = excluded.enabled, \
       updated_at = excluded.updated_at \
     RETURNING id, instrument_id, provider, provider_symbol, currency, enabled, created_at, updated_at";

#[derive(Clone, Debug, sqlx::FromRow)]
pub struct ProviderSymbolRow {
    pub id: i64,
    pub instrument_id: i64,
    pub provider: String,
    pub provider_symbol: String,
    pub currency: Option<String>,
    pub enabled: bool,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Clone, Debug)]
pub struct NewProviderSymbol {
    pub instrument_id: i64,
    pub provider: String,
    pub provider_symbol: String,
    pub currency: Option<String>,
    pub enabled: bool,
    pub created_at: String,
    pub updated_at: String,
}

pub async fn list(pool: &SqlitePool) -> Result<Vec<ProviderSymbolRow>, RepoError> {
    let rows = sqlx::query_as::<_, ProviderSymbolRow>(LIST_SQL)
        .fetch_all(pool)
        .await?;
    Ok(rows)
}

pub async fn find(pool: &SqlitePool, id: i64) -> Result<Option<ProviderSymbolRow>, RepoError> {
    let row = sqlx::query_as::<_, ProviderSymbolRow>(FIND_SQL)
        .bind(id)
        .fetch_optional(pool)
        .await?;
    Ok(row)
}

pub async fn find_by_instrument_provider(
    pool: &SqlitePool,
    instrument_id: i64,
    provider: &str,
) -> Result<Option<ProviderSymbolRow>, RepoError> {
    let row = sqlx::query_as::<_, ProviderSymbolRow>(FIND_BY_INSTRUMENT_PROVIDER_SQL)
        .bind(instrument_id)
        .bind(provider)
        .fetch_optional(pool)
        .await?;
    Ok(row)
}

pub async fn list_by_provider_symbol(
    pool: &SqlitePool,
    provider: &str,
    provider_symbol: &str,
) -> Result<Vec<ProviderSymbolRow>, RepoError> {
    let rows = sqlx::query_as::<_, ProviderSymbolRow>(LIST_BY_PROVIDER_SYMBOL_SQL)
        .bind(provider)
        .bind(provider_symbol)
        .fetch_all(pool)
        .await?;
    Ok(rows)
}

/// Upsert a provider-symbol mapping by `(instrument_id, provider)`.
pub async fn upsert(
    pool: &SqlitePool,
    new: &NewProviderSymbol,
) -> Result<ProviderSymbolRow, RepoError> {
    let row = sqlx::query_as::<_, ProviderSymbolRow>(UPSERT_SQL)
        .bind(new.instrument_id)
        .bind(&new.provider)
        .bind(&new.provider_symbol)
        .bind(new.currency.clone())
        .bind(new.enabled)
        .bind(&new.created_at)
        .bind(&new.updated_at)
        .fetch_one(pool)
        .await?;
    Ok(row)
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::db::{instruments, testing};

    #[tokio::test]
    async fn provider_symbol_upsert_is_idempotent_and_updates_in_place() {
        let pool = testing::memory_pool().await;
        let instrument_id = seed_instrument(&pool, "MSFT").await;

        let first = upsert(
            &pool,
            &NewProviderSymbol {
                instrument_id,
                provider: "YAHOO".to_owned(),
                provider_symbol: "MSFT".to_owned(),
                currency: Some("USD".to_owned()),
                enabled: true,
                created_at: "2026-06-16T08:00:00Z".to_owned(),
                updated_at: "2026-06-16T08:00:00Z".to_owned(),
            },
        )
        .await
        .expect("first upsert should succeed");

        let second = upsert(
            &pool,
            &NewProviderSymbol {
                instrument_id,
                provider: "YAHOO".to_owned(),
                provider_symbol: "MSFTX".to_owned(),
                currency: None,
                enabled: false,
                created_at: "2026-06-16T08:10:00Z".to_owned(),
                updated_at: "2026-06-16T08:10:00Z".to_owned(),
            },
        )
        .await
        .expect("second upsert should succeed");

        assert_eq!(first.id, second.id);
        assert_eq!(second.provider_symbol, "MSFTX");
        assert_eq!(second.currency, None);
        assert!(!second.enabled);
        assert_eq!(second.created_at, "2026-06-16T08:00:00Z");
        assert_eq!(second.updated_at, "2026-06-16T08:10:00Z");

        let rows = list(&pool).await.expect("list should succeed");
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].provider_symbol, "MSFTX");

        let by_pair = find_by_instrument_provider(&pool, instrument_id, "YAHOO")
            .await
            .expect("pair lookup should succeed")
            .expect("row should exist");
        assert_eq!(by_pair.id, second.id);

        let by_symbol = list_by_provider_symbol(&pool, "YAHOO", "MSFTX")
            .await
            .expect("symbol lookup should succeed");
        assert_eq!(by_symbol.len(), 1);
        assert_eq!(by_symbol[0].id, second.id);
    }

    #[tokio::test]
    async fn provider_symbol_reverse_lookup_returns_all_matches() {
        let pool = testing::memory_pool().await;
        let first_instrument_id = seed_instrument(&pool, "MSFT").await;
        let second_instrument_id = seed_instrument(&pool, "MSFT.B").await;

        upsert(
            &pool,
            &NewProviderSymbol {
                instrument_id: first_instrument_id,
                provider: "YAHOO".to_owned(),
                provider_symbol: "MSFT".to_owned(),
                currency: Some("USD".to_owned()),
                enabled: true,
                created_at: "2026-06-16T08:00:00Z".to_owned(),
                updated_at: "2026-06-16T08:00:00Z".to_owned(),
            },
        )
        .await
        .expect("first mapping should upsert");

        upsert(
            &pool,
            &NewProviderSymbol {
                instrument_id: second_instrument_id,
                provider: "YAHOO".to_owned(),
                provider_symbol: "MSFT".to_owned(),
                currency: Some("USD".to_owned()),
                enabled: true,
                created_at: "2026-06-16T08:01:00Z".to_owned(),
                updated_at: "2026-06-16T08:01:00Z".to_owned(),
            },
        )
        .await
        .expect("second mapping should upsert");

        let matches = list_by_provider_symbol(&pool, "YAHOO", "MSFT")
            .await
            .expect("reverse lookup should succeed");

        assert_eq!(matches.len(), 2);
        assert_eq!(matches[0].instrument_id, first_instrument_id);
        assert_eq!(matches[1].instrument_id, second_instrument_id);
    }

    #[tokio::test]
    async fn provider_symbol_constraints_are_enforced() {
        let pool = testing::memory_pool().await;
        let instrument_id = seed_instrument(&pool, "MSFT").await;

        let invalid_provider = sqlx::query(
            "INSERT INTO instrument_provider_symbols (instrument_id, provider, provider_symbol, currency, enabled, created_at, updated_at) VALUES (?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(instrument_id)
        .bind("BAD")
        .bind("MSFT")
        .bind(Some("USD"))
        .bind(1_i64)
        .bind("2026-06-16T08:00:00Z")
        .bind("2026-06-16T08:00:00Z")
        .execute(&pool)
        .await
        .expect_err("invalid provider should fail");

        assert!(is_constraint_error(&invalid_provider, "CHECK"));

        let missing_instrument = sqlx::query(
            "INSERT INTO instrument_provider_symbols (instrument_id, provider, provider_symbol, currency, enabled, created_at, updated_at) VALUES (?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(999_i64)
        .bind("YAHOO")
        .bind("MSFT")
        .bind(Some("USD"))
        .bind(1_i64)
        .bind("2026-06-16T08:00:00Z")
        .bind("2026-06-16T08:00:00Z")
        .execute(&pool)
        .await
        .expect_err("missing instrument should fail");

        assert!(is_constraint_error(&missing_instrument, "FOREIGN KEY"));
    }

    async fn seed_instrument(pool: &sqlx::sqlite::SqlitePool, symbol: &str) -> i64 {
        let instrument = instruments::upsert(
            pool,
            &instruments::NewInstrument {
                symbol: symbol.to_owned(),
                exchange: "NASDAQ".to_owned(),
                name: "Microsoft".to_owned(),
                kind: "STOCK".to_owned(),
                currency: "USD".to_owned(),
            },
        )
        .await
        .expect("instrument upsert should succeed");

        instrument.0.id
    }

    fn is_constraint_error(error: &sqlx::Error, needle: &str) -> bool {
        matches!(error, sqlx::Error::Database(database_error) if database_error.message().contains(needle))
    }
}
