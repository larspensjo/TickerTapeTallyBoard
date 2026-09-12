use std::{
    fs::{self, OpenOptions},
    io,
    path::{Path, PathBuf},
};

use chrono::{DateTime, Utc};
use sqlx::{
    sqlite::{SqliteConnectOptions, SqliteConnection},
    Connection, SqlitePool,
};

use super::{retention::snapshot_file_name, SnapshotKind};

const COLLISION_LIMIT: u32 = 100;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LaunchBackupStatus {
    Succeeded,
    Failed,
    Skipped,
    Disabled,
}

impl LaunchBackupStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Succeeded => "succeeded",
            Self::Failed => "failed",
            Self::Skipped => "skipped",
            Self::Disabled => "disabled",
        }
    }
}

#[derive(Debug, Clone)]
pub struct LaunchBackupOutcome {
    pub status: LaunchBackupStatus,
    pub error: Option<String>,
}
impl LaunchBackupOutcome {
    pub fn succeeded() -> Self {
        Self {
            status: LaunchBackupStatus::Succeeded,
            error: None,
        }
    }
    pub fn failed(error: impl ToString) -> Self {
        Self {
            status: LaunchBackupStatus::Failed,
            error: Some(error.to_string()),
        }
    }
    pub fn skipped() -> Self {
        Self {
            status: LaunchBackupStatus::Skipped,
            error: None,
        }
    }
    pub fn disabled() -> Self {
        Self {
            status: LaunchBackupStatus::Disabled,
            error: None,
        }
    }
}

#[derive(Debug)]
pub struct BackupError {
    pub ledger_path: PathBuf,
    pub backup_directory: PathBuf,
    pub snapshot_path: Option<PathBuf>,
    pub operation: &'static str,
    pub source: String,
}
impl std::fmt::Display for BackupError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "ledger {} backup directory {} snapshot {} during {}: {}",
            self.ledger_path.display(),
            self.backup_directory.display(),
            self.snapshot_path.as_ref().map_or_else(
                || "<not allocated>".to_owned(),
                |path| path.display().to_string()
            ),
            self.operation,
            self.source
        )
    }
}
impl std::error::Error for BackupError {}

pub async fn take_snapshot(
    pool: &SqlitePool,
    ledger_path: &Path,
    directory: &Path,
    kind: SnapshotKind,
) -> Result<PathBuf, Box<BackupError>> {
    let timestamp = crate::clock::now_utc();
    let first_target = directory.join(snapshot_file_name(timestamp, kind, 1));
    fs::create_dir_all(directory).map_err(|error| {
        error_at(
            ledger_path,
            directory,
            &first_target,
            "create backup directory",
            error,
        )
    })?;
    let (partial, final_path) = reserve_name(ledger_path, directory, timestamp, kind)?;
    // SQLite accepts an existing destination when it is empty. Keeping the
    // create_new reservation in place prevents another launch from claiming
    // this name between reservation and VACUUM INTO.
    let partial_text = partial.to_string_lossy().into_owned();
    if let Err(error) = sqlx::query("VACUUM INTO ?1")
        .bind(&partial_text)
        .execute(pool)
        .await
    {
        return fail_partial(ledger_path, directory, &partial, "vacuum into", error);
    }
    if let Err(error) = verify_snapshot(&partial).await {
        return fail_partial(
            ledger_path,
            directory,
            &partial,
            error.operation,
            error.source,
        );
    }
    fs::rename(&partial, &final_path)
        .map_err(|error| error_at(ledger_path, directory, &partial, "finalize snapshot", error))?;
    crate::engine_info!(
        "snapshot written for ledger {} in {} at {}",
        ledger_path.display(),
        directory.display(),
        final_path.display()
    );
    Ok(final_path)
}

fn reserve_name(
    ledger_path: &Path,
    directory: &Path,
    timestamp: DateTime<Utc>,
    kind: SnapshotKind,
) -> Result<(PathBuf, PathBuf), Box<BackupError>> {
    for disambiguator in 1..=COLLISION_LIMIT {
        let final_path = directory.join(snapshot_file_name(timestamp, kind, disambiguator));
        let partial = PathBuf::from(format!("{}.partial", final_path.display()));
        if final_path.exists() {
            continue;
        }
        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&partial)
        {
            Ok(_) => return Ok((partial, final_path)),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => {
                return Err(error_at(
                    ledger_path,
                    directory,
                    &partial,
                    "reserve partial snapshot",
                    error,
                ))
            }
        }
    }
    Err(Box::new(BackupError {
        ledger_path: ledger_path.to_path_buf(),
        backup_directory: directory.to_path_buf(),
        snapshot_path: Some(directory.join(snapshot_file_name(timestamp, kind, COLLISION_LIMIT))),
        operation: "reserve partial snapshot",
        source: format!("collision limit of {COLLISION_LIMIT} reached"),
    }))
}

#[derive(Debug)]
struct VerificationError {
    operation: &'static str,
    source: String,
}

async fn verify_snapshot(path: &Path) -> Result<(), VerificationError> {
    let options = SqliteConnectOptions::new().filename(path).read_only(true);
    let mut connection = SqliteConnection::connect_with(&options)
        .await
        .map_err(|error| VerificationError {
            operation: "open snapshot for integrity check",
            source: error.to_string(),
        })?;
    let verification = verify_open_snapshot(&mut connection).await;
    let close = connection.close().await.map_err(|error| VerificationError {
        operation: "close verified snapshot",
        source: error.to_string(),
    });
    verification?;
    close
}

async fn verify_open_snapshot(connection: &mut SqliteConnection) -> Result<(), VerificationError> {
    let integrity: String = sqlx::query_scalar("PRAGMA integrity_check")
        .fetch_one(&mut *connection)
        .await
        .map_err(|error| VerificationError {
            operation: "integrity check",
            source: error.to_string(),
        })?;
    require_integrity_ok(integrity)?;
    let foreign_key_errors = sqlx::query("PRAGMA foreign_key_check")
        .fetch_all(&mut *connection)
        .await
        .map_err(|error| VerificationError {
            operation: "foreign key check",
            source: error.to_string(),
        })?;
    if !foreign_key_errors.is_empty() {
        return Err(VerificationError {
            operation: "foreign key check",
            source: format!("{} violation(s)", foreign_key_errors.len()),
        });
    }
    Ok(())
}

fn require_integrity_ok(integrity: String) -> Result<(), VerificationError> {
    if integrity == "ok" {
        Ok(())
    } else {
        Err(VerificationError {
            operation: "integrity check",
            source: integrity,
        })
    }
}

fn fail_partial<T: ToString>(
    ledger_path: &Path,
    directory: &Path,
    partial: &Path,
    operation: &'static str,
    error: T,
) -> Result<PathBuf, Box<BackupError>> {
    let failed = PathBuf::from(format!("{}.failed", partial.with_extension("").display()));
    let (snapshot_path, source) = match fs::rename(partial, &failed) {
        Ok(()) => (failed, error.to_string()),
        Err(rename_error) => (
            partial.to_path_buf(),
            format!(
                "{}; could not preserve as {}: {rename_error}",
                error.to_string(),
                failed.display()
            ),
        ),
    };
    Err(Box::new(BackupError {
        ledger_path: ledger_path.to_path_buf(),
        backup_directory: directory.to_path_buf(),
        snapshot_path: Some(snapshot_path),
        operation,
        source,
    }))
}

fn error_at(
    ledger_path: &Path,
    directory: &Path,
    snapshot_path: &Path,
    operation: &'static str,
    error: io::Error,
) -> Box<BackupError> {
    Box::new(BackupError {
        ledger_path: ledger_path.to_path_buf(),
        backup_directory: directory.to_path_buf(),
        snapshot_path: Some(snapshot_path.to_path_buf()),
        operation,
        source: error.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        config::Mode,
        db,
        ledger::{resolve, SnapshotFile},
    };
    use sqlx::sqlite::SqlitePoolOptions;
    use std::time::{Instant, SystemTime, UNIX_EPOCH};

    fn temp_directory(name: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let directory = PathBuf::from("target/test-backups").join(format!("{name}-{unique}"));
        fs::create_dir_all(&directory).unwrap();
        directory
    }

    #[tokio::test]
    async fn snapshot_is_verified_and_partial_does_not_survive() {
        let directory = temp_directory("integrity");
        let ledger = directory.join("ledger.sqlite");
        let location = resolve(
            &format!("sqlite://{}", ledger.to_string_lossy().replace('\\', "/")),
            Mode::Production,
        )
        .unwrap();
        let opened = db::open(&location, db::CreateMissing::Yes).await.unwrap();
        db::migrate(&opened.pool).await.unwrap();
        sqlx::query("CREATE TABLE backup_measurement_payload (payload BLOB NOT NULL)")
            .execute(&opened.pool)
            .await
            .unwrap();
        for _ in 0..4 {
            sqlx::query("INSERT INTO backup_measurement_payload (payload) VALUES (zeroblob(?1))")
                .bind(1_048_576_i64)
                .execute(&opened.pool)
                .await
                .unwrap();
        }
        let page_count: i64 = sqlx::query_scalar("PRAGMA page_count")
            .fetch_one(&opened.pool)
            .await
            .unwrap();
        let page_size: i64 = sqlx::query_scalar("PRAGMA page_size")
            .fetch_one(&opened.pool)
            .await
            .unwrap();
        let logical_size = page_count * page_size;
        assert!((4_000_000..6_000_000).contains(&logical_size));
        let backup_dir = directory.join("backups");
        let started = Instant::now();
        let snapshot = take_snapshot(&opened.pool, &ledger, &backup_dir, SnapshotKind::Launch)
            .await
            .unwrap();
        eprintln!(
            "snapshot plus integrity check for {logical_size} byte ledger: {:?}",
            started.elapsed()
        );
        assert!(snapshot.is_file());
        assert!(verify_snapshot(&snapshot).await.is_ok());
        assert!(fs::read_dir(&backup_dir).unwrap().all(|entry| !entry
            .unwrap()
            .path()
            .to_string_lossy()
            .contains(".partial")));
        opened.pool.close().await;
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn collision_uses_disambiguator_without_overwrite() {
        let directory = temp_directory("collision");
        let timestamp = crate::clock::now_utc();
        let original = directory.join(snapshot_file_name(timestamp, SnapshotKind::Launch, 1));
        fs::write(&original, "existing").unwrap();
        let ledger = directory.join("ledger.sqlite");
        let (partial, final_path) =
            reserve_name(&ledger, &directory, timestamp, SnapshotKind::Launch).unwrap();
        assert!(final_path
            .file_name()
            .unwrap()
            .to_string_lossy()
            .contains("-2.sqlite"));
        assert_eq!(fs::read_to_string(original).unwrap(), "existing");
        fs::remove_file(partial).unwrap();
        fs::remove_dir_all(directory).unwrap();
    }

    #[tokio::test]
    async fn vacuum_writes_into_the_held_empty_reservation() {
        let directory = temp_directory("vacuum-reservation");
        let ledger = directory.join("ledger.sqlite");
        let options = SqliteConnectOptions::new()
            .filename(&ledger)
            .create_if_missing(true);
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(options)
            .await
            .unwrap();
        let target = directory.join("reserved.sqlite.partial");
        fs::write(&target, []).unwrap();

        sqlx::query("VACUUM INTO ?1")
            .bind(target.to_string_lossy().into_owned())
            .execute(&pool)
            .await
            .expect("VACUUM should populate the empty reservation");

        assert!(target.metadata().expect("snapshot metadata").len() > 0);
        assert!(verify_snapshot(&target).await.is_ok());
        pool.close().await;
        fs::remove_dir_all(directory).unwrap();
    }

    #[tokio::test]
    async fn garbage_snapshot_fails_verification() {
        let directory = temp_directory("garbage");
        let snapshot = directory.join("garbage.sqlite");
        fs::write(&snapshot, "not sqlite").unwrap();

        let error = verify_snapshot(&snapshot)
            .await
            .expect_err("garbage must fail verification");

        assert!(!error.source.is_empty());
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn integrity_check_output_other_than_ok_is_rejected() {
        let error = require_integrity_ok("database disk image is malformed".to_owned())
            .expect_err("corruption output must fail verification");

        assert_eq!(error.operation, "integrity check");
        assert_eq!(error.source, "database disk image is malformed");
    }

    #[tokio::test]
    async fn failed_snapshot_is_preserved_without_a_final_named_file() {
        let directory = temp_directory("failed");
        let ledger = directory.join("ledger.sqlite");
        let options = SqliteConnectOptions::new()
            .filename(&ledger)
            .create_if_missing(true)
            .foreign_keys(false);
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(options)
            .await
            .unwrap();
        sqlx::query("CREATE TABLE parent (id INTEGER PRIMARY KEY)")
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("CREATE TABLE child (parent_id INTEGER REFERENCES parent(id))")
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("INSERT INTO child (parent_id) VALUES (999)")
            .execute(&pool)
            .await
            .unwrap();
        let backup_dir = directory.join("backups");

        let error = take_snapshot(&pool, &ledger, &backup_dir, SnapshotKind::Launch)
            .await
            .expect_err("foreign-key-invalid snapshot must fail");

        assert_eq!(error.operation, "foreign key check");
        let message = error.to_string();
        assert!(message.contains(&ledger.display().to_string()));
        assert!(message.contains(&backup_dir.display().to_string()));
        assert!(message.contains("foreign key check"));
        let failed_path = error
            .snapshot_path
            .as_ref()
            .expect("failed artifact path should be reported");
        assert!(failed_path.to_string_lossy().ends_with(".failed"));
        assert!(message.contains(&failed_path.display().to_string()));
        let paths: Vec<_> = fs::read_dir(&backup_dir)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .collect();
        assert_eq!(paths, vec![failed_path.clone()]);
        assert!(!paths.iter().any(|path| {
            SnapshotFile::parse(path).is_some_and(|file| file.kind == SnapshotKind::Launch)
        }));
        pool.close().await;
        fs::remove_dir_all(directory).unwrap();
    }
}
