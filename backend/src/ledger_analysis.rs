//! Safety boundaries for offline ledger analysis. Never used by request or refresh paths.
use std::path::{Path, PathBuf};

use rust_decimal::Decimal;
use sqlx::{
    sqlite::{SqliteConnectOptions, SqlitePoolOptions},
    ConnectOptions, Row, SqlitePool,
};

pub mod replay_inputs;

/// Resolve configuration only; do not open an application ledger or inspect backups.
fn configured_paths() -> Vec<PathBuf> {
    use crate::config::{default_database_url, Mode, DATABASE_URL_ENV};
    let mut urls = Vec::new();
    if let Ok(url) = std::env::var(DATABASE_URL_ENV) {
        urls.push(url);
    }
    for mode in [Mode::Production, Mode::Development] {
        if let Ok(url) = default_database_url(mode) {
            urls.push(url);
        }
    }
    urls.into_iter()
        .filter_map(|url| crate::ledger::resolve(&url, Mode::Production).ok()?.path)
        .collect()
}

/// Opens an existing standalone scratch snapshot, never an application's live ledger.
/// Immutable mode is intentional: no journal recovery and no WAL/SHM sidecar creation.
/// Supply a completed SQLite backup snapshot, not a live WAL database copy.
pub async fn open_scratch_ledger(path: &Path) -> Result<SqlitePool, &'static str> {
    let repository = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .ok_or("Cannot resolve repository boundary.")?;
    let backups = [
        crate::config::Mode::Production,
        crate::config::Mode::Development,
    ]
    .into_iter()
    .filter_map(|mode| {
        crate::config::resolve_backup_dir(mode)
            .path()
            .map(Path::to_path_buf)
    })
    .collect::<Vec<_>>();
    open_checked_with_backups(path, repository, &configured_paths(), &backups).await
}

fn checked_path(
    path: &Path,
    repository: &Path,
    forbidden: &[PathBuf],
    backups: &[PathBuf],
) -> Result<PathBuf, &'static str> {
    let path = path
        .canonicalize()
        .map_err(|_| "Ledger path does not exist or cannot be resolved.")?;
    let repository = repository
        .canonicalize()
        .map_err(|_| "Cannot resolve repository boundary.")?;
    if path.starts_with(repository)
        || path
            .ancestors()
            .skip(1)
            .any(|ancestor| ancestor.join(".git").symlink_metadata().is_ok())
    {
        return Err("Refusing a ledger inside the repository working tree; use a scratch snapshot outside it.");
    }
    if forbidden.iter().any(|live| {
        live.canonicalize()
            .map(|live| live == path)
            .unwrap_or_else(|_| live == &path)
    }) {
        return Err("Refusing an application's configured ledger; use a scratch snapshot copy.");
    }
    if backups.iter().any(|backup| {
        backup
            .canonicalize()
            .is_ok_and(|backup| path.starts_with(backup))
    }) {
        return Err(
            "Refusing a ledger inside a configured backup directory; use a scratch snapshot copy.",
        );
    }
    if !path.is_file() {
        return Err("Ledger path must name an existing regular file.");
    }
    Ok(path)
}

#[cfg(test)]
async fn open_checked(
    path: &Path,
    repository: &Path,
    forbidden: &[PathBuf],
) -> Result<SqlitePool, &'static str> {
    open_checked_with_backups(path, repository, forbidden, &[]).await
}

async fn open_checked_with_backups(
    path: &Path,
    repository: &Path,
    forbidden: &[PathBuf],
    backups: &[PathBuf],
) -> Result<SqlitePool, &'static str> {
    let path = checked_path(path, repository, forbidden, backups)?;
    let options = SqliteConnectOptions::new()
        .filename(path)
        .read_only(true)
        .create_if_missing(false)
        .immutable(true)
        .pragma("query_only", "ON")
        .disable_statement_logging();
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(options)
        .await
        .map_err(|_| "Cannot open scratch snapshot read-only.")?;
    if exact_schema(&pool).await.is_err() {
        pool.close().await;
        return Err("Ledger schema must exactly match this build: missing, failed, unknown or changed migrations are refused. No migrations were run.");
    }
    Ok(pool)
}

async fn exact_schema(pool: &SqlitePool) -> Result<(), ()> {
    let rows =
        sqlx::query("SELECT version, success, checksum FROM _sqlx_migrations ORDER BY version")
            .fetch_all(pool)
            .await
            .map_err(|_| ())?;
    let expected = sqlx::migrate!("./migrations");
    if rows.len() != expected.iter().count() {
        return Err(());
    }
    for (row, migration) in rows.iter().zip(expected.iter()) {
        if row.try_get::<i64, _>("version").map_err(|_| ())? != migration.version
            || !row.try_get::<bool, _>("success").map_err(|_| ())?
            || row.try_get::<Vec<u8>, _>("checksum").map_err(|_| ())? != migration.checksum.as_ref()
        {
            return Err(());
        }
    }
    Ok(())
}

/// Replay output requires both an event floor and an instrument floor.
/// Counts remain printable even when statistics are suppressed.
pub fn replay_statistics_allowed(events: usize, instruments: usize) -> bool {
    events >= 5 && instruments >= 3
}

/// A share's group is its denominator population. Counts remain usable at any size.
pub fn aggregate_share(count: usize, population: usize) -> String {
    if population < 5 {
        "suppressed".to_owned()
    } else {
        format!(
            "{:.2}%",
            (Decimal::from(count) * Decimal::from(100) / Decimal::from(population)).round_dp(2)
        )
    }
}

/// For future offline analyses; callers must never report a small-group median directly.
pub fn aggregate_median(values: &[Decimal]) -> Option<Decimal> {
    if values.len() < 5 {
        return None;
    }
    crate::domain::opportunity_evidence::replay_rules::median(values)
}

/// Nearest-rank tenth percentile: sorted observation ceil(n / 10), one-based.
/// Suppression is applied before any value can reach the tool's renderer.
pub fn aggregate_tenth_percentile(values: &[Decimal]) -> Option<Decimal> {
    if values.len() < 5 {
        return None;
    }
    let mut sorted = values.to_vec();
    sorted.sort_unstable();
    Some(sorted[sorted.len().div_ceil(10) - 1])
}

#[cfg(test)]
mod tests;
