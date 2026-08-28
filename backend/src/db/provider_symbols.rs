use sqlx::sqlite::SqlitePool;

use crate::{db::RepoError, providers::MarketDataProvider};

const LIST_SQL: &str = "SELECT id, instrument_id, provider, provider_symbol, asset_class, currency, enabled, created_at, updated_at \
    FROM instrument_provider_symbols ORDER BY instrument_id, provider";
const FIND_SQL: &str = "SELECT id, instrument_id, provider, provider_symbol, asset_class, currency, enabled, created_at, updated_at \
    FROM instrument_provider_symbols WHERE id = ?";
const FIND_BY_INSTRUMENT_PROVIDER_SQL: &str = "SELECT id, instrument_id, provider, provider_symbol, asset_class, currency, enabled, created_at, updated_at \
    FROM instrument_provider_symbols WHERE instrument_id = ? AND provider = ?";
const LIST_BY_PROVIDER_SYMBOL_SQL: &str = "SELECT id, instrument_id, provider, provider_symbol, asset_class, currency, enabled, created_at, updated_at \
    FROM instrument_provider_symbols WHERE provider = ? AND provider_symbol = ? ORDER BY instrument_id, id";
const DELETE_BY_INSTRUMENT_SQL: &str =
    "DELETE FROM instrument_provider_symbols WHERE instrument_id = ?";
const UPSERT_SQL: &str = "INSERT INTO instrument_provider_symbols \
       (instrument_id, provider, provider_symbol, asset_class, currency, enabled, created_at, updated_at) \
     VALUES (?, ?, ?, ?, ?, ?, ?, ?) \
     ON CONFLICT (instrument_id, provider) DO UPDATE SET \
       provider_symbol = excluded.provider_symbol, \
       asset_class = excluded.asset_class, \
       currency = excluded.currency, \
       enabled = excluded.enabled, \
       updated_at = excluded.updated_at \
     RETURNING id, instrument_id, provider, provider_symbol, asset_class, currency, enabled, created_at, updated_at";

#[derive(Clone, Debug, sqlx::FromRow)]
struct RawProviderSymbolRow {
    pub id: i64,
    pub instrument_id: i64,
    pub provider: String,
    pub provider_symbol: String,
    pub asset_class: Option<String>,
    pub currency: Option<String>,
    pub enabled: bool,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Clone, Debug)]
pub struct ProviderSymbolRow {
    pub id: i64,
    pub instrument_id: i64,
    pub provider: MarketDataProvider,
    pub provider_symbol: String,
    pub asset_class: Option<String>,
    pub currency: Option<String>,
    pub enabled: bool,
    pub created_at: String,
    pub updated_at: String,
}

impl TryFrom<RawProviderSymbolRow> for ProviderSymbolRow {
    type Error = RepoError;

    fn try_from(row: RawProviderSymbolRow) -> Result<Self, Self::Error> {
        let provider = MarketDataProvider::from_db_str(&row.provider).ok_or_else(|| {
            RepoError::Decode(format!(
                "unknown provider-symbol provider {:?} in row {} for instrument {}",
                row.provider, row.id, row.instrument_id
            ))
        })?;
        Ok(Self {
            id: row.id,
            instrument_id: row.instrument_id,
            provider,
            provider_symbol: row.provider_symbol,
            asset_class: row.asset_class,
            currency: row.currency,
            enabled: row.enabled,
            created_at: row.created_at,
            updated_at: row.updated_at,
        })
    }
}

#[derive(Clone, Debug)]
pub struct NewProviderSymbol {
    pub instrument_id: i64,
    pub provider: MarketDataProvider,
    pub provider_symbol: String,
    pub asset_class: Option<String>,
    pub currency: Option<String>,
    pub enabled: bool,
    pub created_at: String,
    pub updated_at: String,
}

pub async fn list(pool: &SqlitePool) -> Result<Vec<ProviderSymbolRow>, RepoError> {
    let rows = sqlx::query_as::<_, RawProviderSymbolRow>(LIST_SQL)
        .fetch_all(pool)
        .await?;
    rows.into_iter().map(TryInto::try_into).collect()
}

pub async fn find(pool: &SqlitePool, id: i64) -> Result<Option<ProviderSymbolRow>, RepoError> {
    let row = sqlx::query_as::<_, RawProviderSymbolRow>(FIND_SQL)
        .bind(id)
        .fetch_optional(pool)
        .await?;
    row.map(TryInto::try_into).transpose()
}

pub async fn find_by_instrument_provider(
    pool: &SqlitePool,
    instrument_id: i64,
    provider: MarketDataProvider,
) -> Result<Option<ProviderSymbolRow>, RepoError> {
    let row = sqlx::query_as::<_, RawProviderSymbolRow>(FIND_BY_INSTRUMENT_PROVIDER_SQL)
        .bind(instrument_id)
        .bind(provider.as_str())
        .fetch_optional(pool)
        .await?;
    row.map(TryInto::try_into).transpose()
}

pub async fn list_by_provider_symbol(
    pool: &SqlitePool,
    provider: MarketDataProvider,
    provider_symbol: &str,
) -> Result<Vec<ProviderSymbolRow>, RepoError> {
    let rows = sqlx::query_as::<_, RawProviderSymbolRow>(LIST_BY_PROVIDER_SYMBOL_SQL)
        .bind(provider.as_str())
        .bind(provider_symbol)
        .fetch_all(pool)
        .await?;
    rows.into_iter().map(TryInto::try_into).collect()
}

/// Upsert a provider-symbol mapping by `(instrument_id, provider)`.
pub async fn upsert(
    pool: &SqlitePool,
    new: &NewProviderSymbol,
) -> Result<ProviderSymbolRow, RepoError> {
    let row = sqlx::query_as::<_, RawProviderSymbolRow>(UPSERT_SQL)
        .bind(new.instrument_id)
        .bind(new.provider.as_str())
        .bind(&new.provider_symbol)
        .bind(new.asset_class.clone())
        .bind(new.currency.clone())
        .bind(new.enabled)
        .bind(&new.created_at)
        .bind(&new.updated_at)
        .fetch_one(pool)
        .await?;
    row.try_into()
}

pub async fn delete_by_instrument_id_in_tx(
    conn: &mut sqlx::sqlite::SqliteConnection,
    instrument_id: i64,
) -> Result<u64, RepoError> {
    let result = sqlx::query(DELETE_BY_INSTRUMENT_SQL)
        .bind(instrument_id)
        .execute(&mut *conn)
        .await?;
    Ok(result.rows_affected())
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
                provider: MarketDataProvider::Yahoo,
                provider_symbol: "MSFT".to_owned(),
                asset_class: None,
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
                provider: MarketDataProvider::Yahoo,
                provider_symbol: "MSFTX".to_owned(),
                asset_class: None,
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

        let by_pair = find_by_instrument_provider(&pool, instrument_id, MarketDataProvider::Yahoo)
            .await
            .expect("pair lookup should succeed")
            .expect("row should exist");
        assert_eq!(by_pair.id, second.id);

        let by_symbol = list_by_provider_symbol(&pool, MarketDataProvider::Yahoo, "MSFTX")
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
                provider: MarketDataProvider::Yahoo,
                provider_symbol: "MSFT".to_owned(),
                asset_class: None,
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
                provider: MarketDataProvider::Yahoo,
                provider_symbol: "MSFT".to_owned(),
                asset_class: None,
                currency: Some("USD".to_owned()),
                enabled: true,
                created_at: "2026-06-16T08:01:00Z".to_owned(),
                updated_at: "2026-06-16T08:01:00Z".to_owned(),
            },
        )
        .await
        .expect("second mapping should upsert");

        let matches = list_by_provider_symbol(&pool, MarketDataProvider::Yahoo, "MSFT")
            .await
            .expect("reverse lookup should succeed");

        assert_eq!(matches.len(), 2);
        assert_eq!(matches[0].instrument_id, first_instrument_id);
        assert_eq!(matches[1].instrument_id, second_instrument_id);
    }

    #[tokio::test]
    async fn unknown_provider_code_is_decode_error_and_foreign_key_is_enforced() {
        let pool = testing::memory_pool().await;
        let instrument_id = seed_instrument(&pool, "MSFT").await;

        sqlx::query(
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
        .expect("unknown provider row inserts after CHECK removal");

        assert!(matches!(list(&pool).await, Err(RepoError::Decode(_))));

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
                isin: None,
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
