use sqlx::sqlite::SqlitePool;

use crate::db::RepoError;

const LIST_SQL: &str = "SELECT id, \"trigger\", started_at, finished_at, status, message \
    FROM market_data_refresh_runs ORDER BY started_at DESC, id DESC";
const FIND_SQL: &str = "SELECT id, \"trigger\", started_at, finished_at, status, message \
    FROM market_data_refresh_runs WHERE id = ?";
const LATEST_SQL: &str = "SELECT id, \"trigger\", started_at, finished_at, status, message \
    FROM market_data_refresh_runs ORDER BY started_at DESC, id DESC LIMIT 1";
const START_SQL: &str = "INSERT INTO market_data_refresh_runs (\"trigger\", started_at, status) \
    VALUES (?, ?, 'RUNNING') RETURNING id, \"trigger\", started_at, finished_at, status, message";
const FINISH_SQL: &str =
    "UPDATE market_data_refresh_runs SET finished_at = ?, status = ?, message = ? \
    WHERE id = ? RETURNING id, \"trigger\", started_at, finished_at, status, message";

#[derive(Clone, Debug, sqlx::FromRow)]
pub struct RefreshRunRow {
    pub id: i64,
    pub trigger: String,
    pub started_at: String,
    pub finished_at: Option<String>,
    pub status: String,
    pub message: Option<String>,
}

pub async fn list(pool: &SqlitePool) -> Result<Vec<RefreshRunRow>, RepoError> {
    let rows = sqlx::query_as::<_, RefreshRunRow>(LIST_SQL)
        .fetch_all(pool)
        .await?;
    Ok(rows)
}

pub async fn find(pool: &SqlitePool, id: i64) -> Result<Option<RefreshRunRow>, RepoError> {
    let row = sqlx::query_as::<_, RefreshRunRow>(FIND_SQL)
        .bind(id)
        .fetch_optional(pool)
        .await?;
    Ok(row)
}

pub async fn latest(pool: &SqlitePool) -> Result<Option<RefreshRunRow>, RepoError> {
    let row = sqlx::query_as::<_, RefreshRunRow>(LATEST_SQL)
        .fetch_optional(pool)
        .await?;
    Ok(row)
}

pub async fn start_run(
    pool: &SqlitePool,
    trigger: &str,
    started_at: &str,
) -> Result<RefreshRunRow, RepoError> {
    let row = sqlx::query_as::<_, RefreshRunRow>(START_SQL)
        .bind(trigger)
        .bind(started_at)
        .fetch_one(pool)
        .await?;
    Ok(row)
}

pub async fn finish_run(
    pool: &SqlitePool,
    id: i64,
    finished_at: &str,
    status: &str,
    message: Option<&str>,
) -> Result<Option<RefreshRunRow>, RepoError> {
    let row = sqlx::query_as::<_, RefreshRunRow>(FINISH_SQL)
        .bind(finished_at)
        .bind(status)
        .bind(message)
        .bind(id)
        .fetch_optional(pool)
        .await?;
    Ok(row)
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::db::testing;

    #[tokio::test]
    async fn refresh_runs_track_lifecycle_and_latest_run() {
        let pool = testing::memory_pool().await;

        let first = start_run(&pool, "LAUNCH", "2026-06-16T08:00:00Z")
            .await
            .expect("first run should start");
        assert_eq!(first.status, "RUNNING");

        let finished = finish_run(
            &pool,
            first.id,
            "2026-06-16T08:02:00Z",
            "SUCCEEDED",
            Some("refreshed 12 rows"),
        )
        .await
        .expect("finish should return a row")
        .expect("finished row should exist");
        assert_eq!(finished.status, "SUCCEEDED");
        assert_eq!(
            finished.finished_at.as_deref(),
            Some("2026-06-16T08:02:00Z")
        );

        let second = start_run(&pool, "MANUAL", "2026-06-16T09:00:00Z")
            .await
            .expect("second run should start");
        assert!(second.id > first.id);

        let latest = latest(&pool)
            .await
            .expect("latest lookup should succeed")
            .expect("latest run should exist");
        assert_eq!(latest.id, second.id);
        assert_eq!(latest.trigger, "MANUAL");

        let listed = list(&pool).await.expect("list should succeed");
        assert_eq!(listed.len(), 2);

        let found = find(&pool, first.id)
            .await
            .expect("find should succeed")
            .expect("row should exist");
        assert_eq!(found.message.as_deref(), Some("refreshed 12 rows"));
    }

    #[tokio::test]
    async fn refresh_run_constraints_are_enforced() {
        let pool = testing::memory_pool().await;

        let invalid_trigger = sqlx::query(
            "INSERT INTO market_data_refresh_runs (\"trigger\", started_at, status) VALUES (?, ?, ?)",
        )
        .bind("BOGUS")
        .bind("2026-06-16T08:00:00Z")
        .bind("RUNNING")
        .execute(&pool)
        .await
        .expect_err("invalid trigger should fail");

        assert!(is_constraint_error(&invalid_trigger, "CHECK"));
    }

    fn is_constraint_error(error: &sqlx::Error, needle: &str) -> bool {
        matches!(error, sqlx::Error::Database(database_error) if database_error.message().contains(needle))
    }
}
