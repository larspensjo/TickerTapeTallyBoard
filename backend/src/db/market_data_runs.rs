use std::time::Duration;

use chrono::{DateTime, Utc};
use futures::future::BoxFuture;
use sqlx::sqlite::{SqliteConnection, SqlitePool};

use crate::{data_revision, db::RepoError};

/// A holder that has not renewed this lease is recoverable by another process.
pub const REFRESH_CLAIM_STALE_AFTER: Duration = Duration::from_secs(120);

const LIST_SQL: &str = "SELECT id, \"trigger\", started_at, finished_at, status, message, \
    prices_written, fx_rates_written, unmapped_instruments, failed_items, claim_owner, heartbeat_at \
    FROM market_data_refresh_runs ORDER BY started_at DESC, id DESC";
const FIND_SQL: &str = "SELECT id, \"trigger\", started_at, finished_at, status, message, \
    prices_written, fx_rates_written, unmapped_instruments, failed_items, claim_owner, heartbeat_at \
    FROM market_data_refresh_runs WHERE id = ?";
const LATEST_SQL: &str = "SELECT id, \"trigger\", started_at, finished_at, status, message, \
    prices_written, fx_rates_written, unmapped_instruments, failed_items, claim_owner, heartbeat_at \
    FROM market_data_refresh_runs ORDER BY started_at DESC, id DESC LIMIT 1";
const RUNNING_SQL: &str = "SELECT id, \"trigger\", started_at, finished_at, status, message, \
    prices_written, fx_rates_written, unmapped_instruments, failed_items, claim_owner, heartbeat_at \
    FROM market_data_refresh_runs WHERE status = 'RUNNING' ORDER BY id DESC";
const INSERT_CLAIM_SQL: &str = "INSERT INTO market_data_refresh_runs (\"trigger\", started_at, status, claim_owner, heartbeat_at) \
    VALUES (?, ?, 'RUNNING', ?, ?) RETURNING id, \"trigger\", started_at, finished_at, status, message, \
    prices_written, fx_rates_written, unmapped_instruments, failed_items, claim_owner, heartbeat_at";

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RefreshRunCounts {
    pub prices_written: i64,
    pub fx_rates_written: i64,
    pub unmapped_instruments: i64,
    pub failed_items: i64,
}
#[derive(Clone, Debug, sqlx::FromRow)]
pub struct RefreshRunRow {
    pub id: i64,
    pub trigger: String,
    pub started_at: String,
    pub finished_at: Option<String>,
    pub status: String,
    pub message: Option<String>,
    pub prices_written: i64,
    pub fx_rates_written: i64,
    pub unmapped_instruments: i64,
    pub failed_items: i64,
    pub claim_owner: Option<String>,
    pub heartbeat_at: Option<String>,
}
#[derive(Clone, Debug)]
pub enum ClaimResult {
    Claimed {
        run: RefreshRunRow,
        reclaimed: Vec<ReclaimedRun>,
    },
    Held(RefreshRunRow),
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReclaimedRun {
    pub id: i64,
    pub owner: String,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LeaseState {
    Held,
    Lost,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FencedWrite {
    LeaseLost,
}

pub async fn list(pool: &SqlitePool) -> Result<Vec<RefreshRunRow>, RepoError> {
    Ok(sqlx::query_as::<_, RefreshRunRow>(LIST_SQL)
        .fetch_all(pool)
        .await?)
}
pub async fn find(pool: &SqlitePool, id: i64) -> Result<Option<RefreshRunRow>, RepoError> {
    Ok(sqlx::query_as::<_, RefreshRunRow>(FIND_SQL)
        .bind(id)
        .fetch_optional(pool)
        .await?)
}
pub async fn latest(pool: &SqlitePool) -> Result<Option<RefreshRunRow>, RepoError> {
    Ok(sqlx::query_as::<_, RefreshRunRow>(LATEST_SQL)
        .fetch_optional(pool)
        .await?)
}

pub async fn try_claim_run(
    pool: &SqlitePool,
    trigger: &str,
    owner: &str,
    now: DateTime<Utc>,
    stale_after: Duration,
) -> Result<ClaimResult, RepoError> {
    let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;
    let running = sqlx::query_as::<_, RefreshRunRow>(RUNNING_SQL)
        .fetch_all(&mut *tx)
        .await?;
    if let Some(held) = running
        .iter()
        .find(|run| !is_stale(run.heartbeat_at.as_deref(), now, stale_after))
    {
        let held = held.clone();
        tx.commit().await?;
        return Ok(ClaimResult::Held(held));
    }

    let reclaimed = running
        .iter()
        .map(|run| ReclaimedRun {
            id: run.id,
            owner: run
                .claim_owner
                .clone()
                .unwrap_or_else(|| "unknown owner".to_owned()),
        })
        .collect::<Vec<_>>();
    if !reclaimed.is_empty() {
        sqlx::query(
            "UPDATE market_data_refresh_runs SET finished_at = ?, status = 'FAILED', \
             message = 'abandoned' WHERE status = 'RUNNING'",
        )
        .bind(now.to_rfc3339())
        .execute(&mut *tx)
        .await?;
        data_revision::bump(&mut *tx).await?;
    }
    let run = insert_claim(&mut tx, trigger, owner, now).await?;
    tx.commit().await?;
    Ok(ClaimResult::Claimed { run, reclaimed })
}
pub async fn heartbeat(
    pool: &SqlitePool,
    id: i64,
    owner: &str,
    now: DateTime<Utc>,
) -> Result<LeaseState, RepoError> {
    let changed = sqlx::query("UPDATE market_data_refresh_runs SET heartbeat_at = ? WHERE id = ? AND claim_owner = ? AND status = 'RUNNING'").bind(now.to_rfc3339()).bind(id).bind(owner).execute(pool).await?.rows_affected();
    Ok(if changed == 1 {
        LeaseState::Held
    } else {
        LeaseState::Lost
    })
}
pub async fn finish_run(
    pool: &SqlitePool,
    id: i64,
    owner: &str,
    finished_at: DateTime<Utc>,
    status: &str,
    message: Option<&str>,
    counts: RefreshRunCounts,
) -> Result<LeaseState, RepoError> {
    let changed = sqlx::query("UPDATE market_data_refresh_runs SET finished_at = ?, status = ?, message = ?, prices_written = ?, fx_rates_written = ?, unmapped_instruments = ?, failed_items = ? WHERE id = ? AND claim_owner = ? AND status = 'RUNNING'").bind(finished_at.to_rfc3339()).bind(status).bind(message).bind(counts.prices_written).bind(counts.fx_rates_written).bind(counts.unmapped_instruments).bind(counts.failed_items).bind(id).bind(owner).execute(pool).await?.rows_affected();
    Ok(if changed == 1 {
        LeaseState::Held
    } else {
        LeaseState::Lost
    })
}

/// Best-effort cleanup for a refresh future that was cancelled after claiming.
/// The revision bump shares the owner-conditional transaction so a cancelled
/// request cannot hide refresh writes from other processes.
pub async fn cancel_run(
    pool: &SqlitePool,
    id: i64,
    owner: &str,
    finished_at: DateTime<Utc>,
) -> Result<LeaseState, RepoError> {
    let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;
    let changed = sqlx::query(
        "UPDATE market_data_refresh_runs SET finished_at = ?, status = 'FAILED', \
         message = 'cancelled' WHERE id = ? AND claim_owner = ? AND status = 'RUNNING'",
    )
    .bind(finished_at.to_rfc3339())
    .bind(id)
    .bind(owner)
    .execute(&mut *tx)
    .await?
    .rows_affected();
    if changed == 1 {
        data_revision::bump(&mut *tx).await?;
    }
    tx.commit().await?;
    Ok(if changed == 1 {
        LeaseState::Held
    } else {
        LeaseState::Lost
    })
}
pub async fn live_claim(
    pool: &SqlitePool,
    now: DateTime<Utc>,
    stale_after: Duration,
) -> Result<Option<RefreshRunRow>, RepoError> {
    let rows = sqlx::query_as::<_, RefreshRunRow>(RUNNING_SQL)
        .fetch_all(pool)
        .await?;
    Ok(rows
        .into_iter()
        .find(|run| !is_stale(run.heartbeat_at.as_deref(), now, stale_after)))
}

pub async fn fenced_write<T, F>(
    pool: &SqlitePool,
    id: i64,
    owner: &str,
    write: F,
) -> Result<Result<T, FencedWrite>, RepoError>
where
    F: for<'connection> FnOnce(
        &'connection mut SqliteConnection,
    ) -> BoxFuture<'connection, Result<T, RepoError>>,
{
    let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;
    let held: Option<i64> = sqlx::query_scalar(
        "SELECT 1 FROM market_data_refresh_runs \
         WHERE id = ? AND claim_owner = ? AND status = 'RUNNING'",
    )
    .bind(id)
    .bind(owner)
    .fetch_optional(&mut *tx)
    .await?;
    if held.is_none() {
        return Ok(Err(FencedWrite::LeaseLost));
    }

    let value = write(&mut tx).await?;
    tx.commit().await?;
    Ok(Ok(value))
}
async fn insert_claim(
    conn: &mut SqliteConnection,
    trigger: &str,
    owner: &str,
    now: DateTime<Utc>,
) -> Result<RefreshRunRow, sqlx::Error> {
    sqlx::query_as::<_, RefreshRunRow>(INSERT_CLAIM_SQL)
        .bind(trigger)
        .bind(now.to_rfc3339())
        .bind(owner)
        .bind(now.to_rfc3339())
        .fetch_one(&mut *conn)
        .await
}
fn is_stale(heartbeat: Option<&str>, now: DateTime<Utc>, stale_after: Duration) -> bool {
    heartbeat
        .and_then(|value| DateTime::parse_from_rfc3339(value).ok())
        .map(|value| {
            now.signed_duration_since(value.with_timezone(&Utc))
                .to_std()
                .unwrap_or_default()
                > stale_after
        })
        .unwrap_or(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;
    use std::{
        fs,
        path::PathBuf,
        time::{SystemTime, UNIX_EPOCH},
    };

    async fn two_file_pools() -> (SqlitePool, SqlitePool, PathBuf) {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let path = crate::test_support::workspace_target_path("refresh-lease-tests")
            .join(format!("{nonce}.sqlite"));
        fs::create_dir_all(path.parent().expect("parent")).expect("test parent");
        let location = crate::ledger::resolve(
            &format!("sqlite://{}", path.to_string_lossy().replace('\\', "/")),
            crate::config::Mode::Production,
        )
        .expect("location");
        let first = crate::db::open(&location, crate::db::CreateMissing::Yes)
            .await
            .expect("first pool");
        crate::db::migrate(&first.pool).await.expect("migrate");
        let second = crate::db::open(&location, crate::db::CreateMissing::No)
            .await
            .expect("second pool");
        (first.pool, second.pool, path)
    }

    #[tokio::test]
    async fn refresh_runs_track_lifecycle_and_latest_run() {
        let pool = crate::db::testing::memory_pool().await;
        let first_at = Utc
            .with_ymd_and_hms(2026, 6, 16, 8, 0, 0)
            .single()
            .expect("time");
        let first = match try_claim_run(
            &pool,
            "LAUNCH",
            "owner-a",
            first_at,
            REFRESH_CLAIM_STALE_AFTER,
        )
        .await
        .expect("first run should start")
        {
            ClaimResult::Claimed { run, .. } => run,
            ClaimResult::Held(_) => panic!("first run unexpectedly held"),
        };
        assert_eq!(first.status, "RUNNING");

        assert_eq!(
            finish_run(
                &pool,
                first.id,
                "owner-a",
                first_at + chrono::Duration::minutes(2),
                "SUCCEEDED",
                Some("refreshed 12 rows"),
                RefreshRunCounts {
                    prices_written: 12,
                    fx_rates_written: 3,
                    unmapped_instruments: 1,
                    failed_items: 0,
                },
            )
            .await
            .expect("finish should succeed"),
            LeaseState::Held
        );
        let finished = find(&pool, first.id)
            .await
            .expect("find should succeed")
            .expect("finished row should exist");
        assert_eq!(finished.status, "SUCCEEDED");
        assert_eq!(finished.prices_written, 12);
        assert_eq!(finished.fx_rates_written, 3);
        assert_eq!(finished.unmapped_instruments, 1);
        assert_eq!(finished.failed_items, 0);

        let second = match try_claim_run(
            &pool,
            "MANUAL",
            "owner-b",
            first_at + chrono::Duration::hours(1),
            REFRESH_CLAIM_STALE_AFTER,
        )
        .await
        .expect("second run should start")
        {
            ClaimResult::Claimed { run, .. } => run,
            ClaimResult::Held(_) => panic!("second run unexpectedly held"),
        };
        assert!(second.id > first.id);
        assert_eq!(latest(&pool).await.unwrap().unwrap().id, second.id);
        assert_eq!(list(&pool).await.expect("list should succeed").len(), 2);
        assert_eq!(
            find(&pool, first.id)
                .await
                .unwrap()
                .unwrap()
                .message
                .as_deref(),
            Some("refreshed 12 rows")
        );
    }

    #[tokio::test]
    async fn refresh_run_constraints_are_enforced() {
        let pool = crate::db::testing::memory_pool().await;
        let invalid_trigger = sqlx::query(
            "INSERT INTO market_data_refresh_runs (\"trigger\", started_at, status) VALUES (?, ?, ?)",
        )
        .bind("BOGUS")
        .bind("2026-06-16T08:00:00Z")
        .bind("RUNNING")
        .execute(&pool)
        .await
        .expect_err("invalid trigger should fail");
        assert!(
            matches!(invalid_trigger, sqlx::Error::Database(error) if error.message().contains("CHECK"))
        );
    }

    #[tokio::test]
    async fn failed_fenced_write_rolls_back_before_returning_connection_to_pool() {
        let pool = crate::db::testing::memory_pool().await;
        let now = Utc
            .with_ymd_and_hms(2026, 9, 18, 10, 0, 0)
            .single()
            .expect("time");
        let run = match try_claim_run(&pool, "MANUAL", "owner-a", now, REFRESH_CLAIM_STALE_AFTER)
            .await
            .expect("claim")
        {
            ClaimResult::Claimed { run, .. } => run,
            ClaimResult::Held(_) => panic!("claim unexpectedly held"),
        };
        let before = data_revision::current(&pool).await.expect("revision");

        let error = fenced_write(&pool, run.id, "owner-a", |conn| {
            Box::pin(async move {
                data_revision::bump(&mut *conn).await?;
                Err::<(), _>(RepoError::Decode("forced failure".to_owned()))
            })
        })
        .await
        .expect_err("write should fail");
        assert!(matches!(error, RepoError::Decode(_)));
        assert_eq!(data_revision::current(&pool).await.unwrap(), before);

        let mut tx = pool
            .begin()
            .await
            .expect("connection is not left in a transaction");
        data_revision::bump(&mut *tx)
            .await
            .expect("writer is not locked");
        tx.commit().await.expect("commit succeeds");
    }

    #[tokio::test]
    async fn reclaim_at_the_fenced_write_boundary_never_commits_a_partial_batch() {
        let (first, second, path) = two_file_pools().await;
        sqlx::query("CREATE TABLE fence_probe (slot INTEGER PRIMARY KEY, value TEXT NOT NULL)")
            .execute(&first)
            .await
            .expect("probe table");
        sqlx::query("INSERT INTO fence_probe (slot, value) VALUES (1, 'initial'), (2, 'initial')")
            .execute(&first)
            .await
            .expect("probe rows");
        let now = Utc
            .with_ymd_and_hms(2026, 9, 18, 10, 0, 0)
            .single()
            .expect("time");
        let run = match try_claim_run(&first, "MANUAL", "owner-a", now, REFRESH_CLAIM_STALE_AFTER)
            .await
            .expect("claim")
        {
            ClaimResult::Claimed { run, .. } => run,
            ClaimResult::Held(_) => panic!("claim unexpectedly held"),
        };
        let (entered_tx, entered_rx) = tokio::sync::oneshot::channel();
        let release = std::sync::Arc::new(tokio::sync::Notify::new());
        let writer = {
            let pool = first.clone();
            let release = std::sync::Arc::clone(&release);
            tokio::spawn(async move {
                fenced_write(&pool, run.id, "owner-a", |conn| {
                    Box::pin(async move {
                        let _ = entered_tx.send(());
                        release.notified().await;
                        sqlx::query("UPDATE fence_probe SET value = 'owner-a' WHERE slot = 1")
                            .execute(&mut *conn)
                            .await?;
                        sqlx::query("UPDATE fence_probe SET value = 'owner-a' WHERE slot = 2")
                            .execute(&mut *conn)
                            .await?;
                        Ok(())
                    })
                })
                .await
            })
        };
        entered_rx.await.expect("fenced transaction entered");
        let reclaimer = {
            let pool = second.clone();
            tokio::spawn(async move {
                try_claim_run(
                    &pool,
                    "MANUAL",
                    "owner-b",
                    now + chrono::Duration::seconds(121),
                    REFRESH_CLAIM_STALE_AFTER,
                )
                .await
            })
        };
        tokio::task::yield_now().await;
        release.notify_waiters();
        assert!(matches!(
            writer.await.expect("writer joins").expect("writer result"),
            Ok(())
        ));
        assert!(matches!(
            reclaimer.await.expect("reclaimer joins").expect("reclaim"),
            ClaimResult::Claimed { .. }
        ));
        let values: Vec<String> = sqlx::query_scalar("SELECT value FROM fence_probe ORDER BY slot")
            .fetch_all(&second)
            .await
            .expect("probe values");
        assert!(
            values == ["owner-a", "owner-a"] || values == ["initial", "initial"],
            "ownership check and batch must serialize as one outcome: {values:?}"
        );
        first.close().await;
        second.close().await;
        let _ = fs::remove_file(path);
    }

    #[tokio::test]
    async fn independent_pools_share_the_claim_and_reclaim_stale_owners() {
        let (a, b, path) = two_file_pools().await;
        let now = Utc
            .with_ymd_and_hms(2026, 9, 18, 10, 0, 0)
            .single()
            .expect("time");
        let first = try_claim_run(&a, "MANUAL", "owner-a", now, REFRESH_CLAIM_STALE_AFTER)
            .await
            .expect("claim");
        let first_id = match first {
            ClaimResult::Claimed { run, .. } => run.id,
            ClaimResult::Held(_) => panic!("first claim held"),
        };
        assert!(matches!(
            try_claim_run(&b, "MANUAL", "owner-b", now, REFRESH_CLAIM_STALE_AFTER)
                .await
                .expect("second claim"),
            ClaimResult::Held(_)
        ));
        assert_eq!(
            heartbeat(
                &a,
                first_id,
                "owner-a",
                now + chrono::Duration::seconds(119)
            )
            .await
            .expect("heartbeat"),
            LeaseState::Held
        );
        assert!(matches!(
            try_claim_run(
                &b,
                "MANUAL",
                "owner-b",
                now + chrono::Duration::seconds(121),
                REFRESH_CLAIM_STALE_AFTER
            )
            .await
            .expect("fresh claim remains held"),
            ClaimResult::Held(_)
        ));
        let revision_before = data_revision::current(&a).await.expect("revision reads");
        let reclaimed = try_claim_run(
            &b,
            "MANUAL",
            "owner-b",
            now + chrono::Duration::seconds(241),
            REFRESH_CLAIM_STALE_AFTER,
        )
        .await
        .expect("reclaim");
        let second_id = match reclaimed {
            ClaimResult::Claimed { run, reclaimed } => {
                assert_eq!(
                    reclaimed,
                    vec![ReclaimedRun {
                        id: first_id,
                        owner: "owner-a".to_owned()
                    }]
                );
                run.id
            }
            ClaimResult::Held(_) => panic!("stale claim held"),
        };
        let abandoned = find(&a, first_id).await.expect("find").expect("row");
        assert_eq!(abandoned.status, "FAILED");
        assert_eq!(abandoned.message.as_deref(), Some("abandoned"));
        assert!(abandoned.finished_at.is_some());
        assert_ne!(
            data_revision::current(&b).await.expect("revision reads"),
            revision_before
        );
        assert_eq!(
            heartbeat(&a, first_id, "owner-a", now)
                .await
                .expect("old heartbeat"),
            LeaseState::Lost
        );
        assert_eq!(
            finish_run(
                &a,
                first_id,
                "owner-a",
                now,
                "SUCCEEDED",
                None,
                RefreshRunCounts::default()
            )
            .await
            .expect("old finish"),
            LeaseState::Lost
        );
        assert_eq!(
            find(&b, second_id)
                .await
                .expect("find")
                .expect("row")
                .status,
            "RUNNING"
        );
        a.close().await;
        b.close().await;
        let _ = fs::remove_file(path);
    }

    #[tokio::test]
    async fn one_claim_reclaims_all_stale_orphan_rows_including_unknown_owners() {
        let pool = crate::db::testing::memory_pool().await;
        for started_at in ["2026-09-18T09:00:00Z", "2026-09-18T09:01:00Z"] {
            sqlx::query(
                "INSERT INTO market_data_refresh_runs (\"trigger\", started_at, status) \
                 VALUES ('MANUAL', ?, 'RUNNING')",
            )
            .bind(started_at)
            .execute(&pool)
            .await
            .expect("orphan row");
        }
        let before = data_revision::current(&pool).await.expect("revision");
        let now = Utc
            .with_ymd_and_hms(2026, 9, 18, 10, 0, 0)
            .single()
            .expect("time");
        let reclaimed = try_claim_run(&pool, "MANUAL", "owner-b", now, REFRESH_CLAIM_STALE_AFTER)
            .await
            .expect("claim");
        let reclaimed = match reclaimed {
            ClaimResult::Claimed { reclaimed, .. } => reclaimed,
            ClaimResult::Held(_) => panic!("orphans should be stale"),
        };
        assert_eq!(reclaimed.len(), 2);
        assert!(reclaimed.iter().all(|row| row.owner == "unknown owner"));
        let still_running: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM market_data_refresh_runs \
             WHERE status = 'RUNNING' AND claim_owner IS NULL",
        )
        .fetch_one(&pool)
        .await
        .expect("count");
        assert_eq!(still_running, 0);
        assert_ne!(data_revision::current(&pool).await.unwrap(), before);
    }
}
