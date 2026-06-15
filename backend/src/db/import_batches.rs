use sqlx::sqlite::SqlitePool;

use crate::db::RepoError;

#[derive(Clone, Debug, sqlx::FromRow)]
pub struct ImportBatchRow {
    pub id: i64,
    pub source: String,
    pub imported_at: String,
    pub raw_file_hash: Option<String>,
}

const COLUMNS: &str = "id, source, imported_at, raw_file_hash";

/// First existing batch whose `raw_file_hash` matches, if any.
pub async fn find_by_hash(
    pool: &SqlitePool,
    hash: &str,
) -> Result<Option<ImportBatchRow>, RepoError> {
    let row = sqlx::query_as::<_, ImportBatchRow>(&format!(
        "SELECT {COLUMNS} FROM import_batches WHERE raw_file_hash = ? ORDER BY id LIMIT 1"
    ))
    .bind(hash)
    .fetch_optional(pool)
    .await?;
    Ok(row)
}
