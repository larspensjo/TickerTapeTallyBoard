mod pool;

use std::str::FromStr;

use sqlx::sqlite::{SqliteConnectOptions, SqlitePool, SqlitePoolOptions};

pub mod fx_rates;
pub mod import_batches;
pub mod instruments;
pub mod market_data_runs;
pub mod prices;
pub mod provider_symbols;
pub mod transactions;

pub mod testing;

pub use pool::connect;

/// A migrated single-connection in-memory pool, for examples and integration tests.
pub async fn memory_pool() -> Result<SqlitePool, sqlx::Error> {
    let options = SqliteConnectOptions::from_str("sqlite::memory:")?.foreign_keys(true);

    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(options)
        .await?;

    sqlx::migrate!("./migrations").run(&pool).await?;
    Ok(pool)
}

/// Errors a repository can surface: a SQL/driver failure, or a failure to decode
/// stored data back into domain types (an internal invariant violation).
#[derive(Debug)]
pub enum RepoError {
    Sqlx(sqlx::Error),
    Decode(String),
}

impl std::fmt::Display for RepoError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Sqlx(error) => write!(f, "database error: {error}"),
            Self::Decode(message) => write!(f, "decode error: {message}"),
        }
    }
}

impl std::error::Error for RepoError {}

impl From<sqlx::Error> for RepoError {
    fn from(error: sqlx::Error) -> Self {
        Self::Sqlx(error)
    }
}
