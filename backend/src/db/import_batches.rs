use sqlx::sqlite::{SqliteConnection, SqlitePool};

use crate::db::RepoError;

#[derive(Clone, Debug, sqlx::FromRow)]
pub struct ImportBatchRow {
    pub id: i64,
    pub source: String,
    pub imported_at: String,
    pub raw_file_hash: Option<String>,
}

const FIND_BY_HASH_SQL: &str =
    "SELECT id, source, imported_at, raw_file_hash FROM import_batches WHERE raw_file_hash = ? ORDER BY id LIMIT 1";
const FIND_BY_ID_SQL: &str =
    "SELECT id, source, imported_at, raw_file_hash FROM import_batches WHERE id = ?";
const DELETE_IN_TX_SQL: &str = "DELETE FROM import_batches WHERE id = ?";

/// First existing batch whose `raw_file_hash` matches, if any.
pub async fn find_by_hash(
    pool: &SqlitePool,
    hash: &str,
) -> Result<Option<ImportBatchRow>, RepoError> {
    let row = sqlx::query_as::<_, ImportBatchRow>(FIND_BY_HASH_SQL)
        .bind(hash)
        .fetch_optional(pool)
        .await?;
    Ok(row)
}

pub async fn find(pool: &SqlitePool, id: i64) -> Result<Option<ImportBatchRow>, RepoError> {
    let row = sqlx::query_as::<_, ImportBatchRow>(FIND_BY_ID_SQL)
        .bind(id)
        .fetch_optional(pool)
        .await?;
    Ok(row)
}

/// Insert a batch row inside a transaction; returns the new batch id.
pub async fn insert_in_tx(
    conn: &mut SqliteConnection,
    source: &str,
    imported_at: &str,
    raw_file_hash: &str,
) -> Result<i64, RepoError> {
    let row = sqlx::query_as::<_, (i64,)>(
        "INSERT INTO import_batches (source, imported_at, raw_file_hash) VALUES (?, ?, ?) RETURNING id",
    )
    .bind(source)
    .bind(imported_at)
    .bind(raw_file_hash)
    .fetch_one(&mut *conn)
    .await?;
    Ok(row.0)
}

/// Delete the batch row itself inside a caller-managed transaction.
pub async fn delete_in_tx(conn: &mut SqliteConnection, id: i64) -> Result<u64, RepoError> {
    let result = sqlx::query(DELETE_IN_TX_SQL)
        .bind(id)
        .execute(&mut *conn)
        .await?;
    Ok(result.rows_affected())
}
