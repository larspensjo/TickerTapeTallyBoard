//! Test-only database helpers shared across the crate's `#[cfg(test)]` modules.

use sqlx::sqlite::SqlitePool;

/// A migrated in-memory SQLite pool. Uses a single connection so the in-memory
/// database (which is per-connection) persists for the pool's lifetime.
pub async fn memory_pool() -> SqlitePool {
    super::memory_pool()
        .await
        .expect("in-memory pool should connect and migrate")
}

#[cfg(test)]
mod tests {
    use super::memory_pool;

    #[tokio::test]
    async fn memory_pool_applies_schema_and_constraints() {
        let pool = memory_pool().await;

        sqlx::query(
            "INSERT INTO instruments (symbol, exchange, name, type, currency) VALUES (?, ?, ?, ?, ?)",
        )
        .bind("MSFT")
        .bind("NASDAQ")
        .bind("Microsoft")
        .bind("STOCK")
        .bind("USD")
        .execute(&pool)
        .await
        .expect("instrument insert should succeed");

        let duplicate_instrument = sqlx::query(
            "INSERT INTO instruments (symbol, exchange, name, type, currency) VALUES (?, ?, ?, ?, ?)",
        )
        .bind("MSFT")
        .bind("NASDAQ")
        .bind("Microsoft Corp")
        .bind("STOCK")
        .bind("USD")
        .execute(&pool)
        .await
        .expect_err("duplicate exchange/symbol should fail");

        assert!(is_constraint_error(&duplicate_instrument, "UNIQUE"));

        let invalid_instrument_type = sqlx::query(
            "INSERT INTO instruments (symbol, exchange, name, type, currency) VALUES (?, ?, ?, ?, ?)",
        )
        .bind("AAPL")
        .bind("NASDAQ")
        .bind("Apple")
        .bind("BOND")
        .bind("USD")
        .execute(&pool)
        .await
        .expect_err("invalid instrument type should fail");

        assert!(is_constraint_error(&invalid_instrument_type, "CHECK"));

        sqlx::query(
            "INSERT INTO import_batches (source, imported_at, raw_file_hash) VALUES (?, ?, ?)",
        )
        .bind("MANUAL")
        .bind("2026-06-14T00:00:00Z")
        .bind(Option::<&str>::None)
        .execute(&pool)
        .await
        .expect("import batch insert should succeed");

        let foreign_key_error = sqlx::query(
            "INSERT INTO transactions (instrument_id, type, trade_date, quantity, price, currency, fx_rate_to_base, brokerage, brokerage_currency, source_value, source_currency, note, import_batch_id) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(999_i64)
        .bind("BUY")
        .bind("2026-06-14")
        .bind(10_i64)
        .bind(Some("12.50"))
        .bind(Some("USD"))
        .bind(Option::<&str>::None)
        .bind(Option::<&str>::None)
        .bind(Option::<&str>::None)
        .bind(Option::<&str>::None)
        .bind(Option::<&str>::None)
        .bind(Option::<&str>::None)
        .bind(Option::<i64>::None)
        .execute(&pool)
        .await
        .expect_err("missing instrument should fail");

        assert!(is_constraint_error(&foreign_key_error, "FOREIGN KEY"));

        let invalid_transaction_type = sqlx::query(
            "INSERT INTO transactions (instrument_id, type, trade_date, quantity, price, currency, fx_rate_to_base, brokerage, brokerage_currency, source_value, source_currency, note, import_batch_id) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(1_i64)
        .bind("BONUS")
        .bind("2026-06-14")
        .bind(10_i64)
        .bind(Some("12.50"))
        .bind(Some("USD"))
        .bind(Option::<&str>::None)
        .bind(Option::<&str>::None)
        .bind(Option::<&str>::None)
        .bind(Option::<&str>::None)
        .bind(Option::<&str>::None)
        .bind(Option::<&str>::None)
        .bind(Option::<i64>::None)
        .execute(&pool)
        .await
        .expect_err("invalid transaction type should fail");

        assert!(is_constraint_error(&invalid_transaction_type, "CHECK"));

        let invalid_batch_source = sqlx::query(
            "INSERT INTO import_batches (source, imported_at, raw_file_hash) VALUES (?, ?, ?)",
        )
        .bind("EMAIL")
        .bind("2026-06-14T00:00:00Z")
        .bind(Option::<&str>::None)
        .execute(&pool)
        .await
        .expect_err("invalid import source should fail");

        assert!(is_constraint_error(&invalid_batch_source, "CHECK"));
    }

    fn is_constraint_error(error: &sqlx::Error, needle: &str) -> bool {
        matches!(error, sqlx::Error::Database(database_error) if database_error.message().contains(needle))
    }
}
