//! Rebuildable per-mapping history request bookkeeping. Callers fence mutations.
use chrono::NaiveDate;
use sqlx::{sqlite::SqliteConnection, SqlitePool};

use super::RepoError;

#[derive(Debug, sqlx::FromRow)]
pub struct SuccessfulAttempt {
    pub remaining_needs: Option<String>,
}

pub async fn attempted_on(
    pool: &SqlitePool,
    mapping_id: i64,
    day: NaiveDate,
) -> Result<bool, RepoError> {
    Ok(sqlx::query_scalar::<_, i64>(
        "SELECT 1 FROM history_topup_attempts WHERE mapping_id=? AND attempted_on=?",
    )
    .bind(mapping_id)
    .bind(day.to_string())
    .fetch_optional(pool)
    .await?
    .is_some())
}

pub async fn last_success(
    pool: &SqlitePool,
    mapping_id: i64,
) -> Result<Option<SuccessfulAttempt>, RepoError> {
    Ok(sqlx::query_as("SELECT remaining_needs FROM history_topup_attempts WHERE mapping_id=? AND outcome='SUCCEEDED' ORDER BY attempted_on DESC LIMIT 1")
        .bind(mapping_id).fetch_optional(pool).await?)
}

/// Reserve before provider I/O, so an interrupted or failed fetch still counts today.
pub async fn begin(
    conn: &mut SqliteConnection,
    mapping_id: i64,
    day: NaiveDate,
    start: NaiveDate,
    end: NaiveDate,
) -> Result<bool, RepoError> {
    Ok(sqlx::query("INSERT INTO history_topup_attempts (mapping_id, attempted_on, requested_start, requested_end, outcome) VALUES (?, ?, ?, ?, 'PENDING') ON CONFLICT (mapping_id, attempted_on) DO NOTHING")
        .bind(mapping_id).bind(day.to_string()).bind(start.to_string()).bind(end.to_string())
        .execute(conn).await?.rows_affected() == 1)
}

pub async fn finish(
    conn: &mut SqliteConnection,
    mapping_id: i64,
    day: NaiveDate,
    earliest: Option<NaiveDate>,
    remaining_needs: Option<&str>,
) -> Result<(), RepoError> {
    sqlx::query("UPDATE history_topup_attempts SET outcome=?, earliest_returned_date=?, remaining_needs=? WHERE mapping_id=? AND attempted_on=?")
        .bind(if remaining_needs.is_some() { "SUCCEEDED" } else { "FAILED" })
        .bind(earliest.map(|date| date.to_string())).bind(remaining_needs)
        .bind(mapping_id).bind(day.to_string()).execute(conn).await?;
    Ok(())
}

pub async fn mapping_is_current(
    conn: &mut SqliteConnection,
    mapping: &super::provider_symbols::ProviderSymbolRow,
) -> Result<bool, RepoError> {
    Ok(sqlx::query_scalar::<_, i64>("SELECT 1 FROM instrument_provider_symbols WHERE id=? AND provider=? AND provider_symbol=? AND asset_class IS ? AND currency IS ? AND enabled=1")
        .bind(mapping.id).bind(mapping.provider.as_str()).bind(&mapping.provider_symbol)
        .bind(&mapping.asset_class).bind(&mapping.currency).fetch_optional(conn).await?.is_some())
}

#[cfg(test)]
mod tests;
