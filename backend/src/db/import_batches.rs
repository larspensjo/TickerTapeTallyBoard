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
const FIND_LATEST_BY_SOURCE_SQL: &str =
    "SELECT id, source, imported_at, raw_file_hash FROM import_batches WHERE source = ? ORDER BY id DESC LIMIT 1";
const COUNT_BY_SOURCE_SQL: &str = "SELECT COUNT(*) FROM import_batches WHERE source = ?";
const DELETE_IN_TX_SQL: &str = "DELETE FROM import_batches WHERE id = ?";
const UPDATE_METADATA_IN_TX_SQL: &str =
    "UPDATE import_batches SET imported_at = ?, raw_file_hash = ? WHERE id = ?";

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

/// Most-recent batch for a given source (highest `id`), if any.
pub async fn find_latest_by_source(
    pool: &SqlitePool,
    source: &str,
) -> Result<Option<ImportBatchRow>, RepoError> {
    let row = sqlx::query_as::<_, ImportBatchRow>(FIND_LATEST_BY_SOURCE_SQL)
        .bind(source)
        .fetch_optional(pool)
        .await?;
    Ok(row)
}

/// Count of live batches for a given source.
pub async fn count_by_source(pool: &SqlitePool, source: &str) -> Result<i64, RepoError> {
    let count: i64 = sqlx::query_scalar(COUNT_BY_SOURCE_SQL)
        .bind(source)
        .fetch_one(pool)
        .await?;
    Ok(count)
}

/// Look up a batch by id inside a caller-managed transaction.
pub async fn find_in_tx(
    conn: &mut SqliteConnection,
    id: i64,
) -> Result<Option<ImportBatchRow>, RepoError> {
    let row = sqlx::query_as::<_, ImportBatchRow>(FIND_BY_ID_SQL)
        .bind(id)
        .fetch_optional(&mut *conn)
        .await?;
    Ok(row)
}

/// Most-recent batch for a given source inside a caller-managed transaction.
pub async fn find_latest_by_source_in_tx(
    conn: &mut SqliteConnection,
    source: &str,
) -> Result<Option<ImportBatchRow>, RepoError> {
    let row = sqlx::query_as::<_, ImportBatchRow>(FIND_LATEST_BY_SOURCE_SQL)
        .bind(source)
        .fetch_optional(&mut *conn)
        .await?;
    Ok(row)
}

/// Update the hash and timestamp of an existing batch in place inside a transaction.
pub async fn update_metadata_in_tx(
    conn: &mut SqliteConnection,
    id: i64,
    imported_at: &str,
    raw_file_hash: &str,
) -> Result<(), RepoError> {
    sqlx::query(UPDATE_METADATA_IN_TX_SQL)
        .bind(imported_at)
        .bind(raw_file_hash)
        .bind(id)
        .execute(&mut *conn)
        .await?;
    Ok(())
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
