use crate::{
    config::AppConfig,
    db::{self, CreateMissing},
    ledger::{
        plan_retention, take_snapshot, BackupError, LaunchBackupOutcome, RetentionPolicy,
        SnapshotFile, SnapshotKind,
    },
    startup_error::StartupError,
};

#[derive(Debug)]
pub struct OpenedLedgerWithBackup {
    pub pool: sqlx::sqlite::SqlitePool,
    pub created_now: bool,
    pub launch_backup: LaunchBackupOutcome,
}

pub async fn open(config: &AppConfig) -> Result<OpenedLedgerWithBackup, StartupError> {
    let create = if config.create_ledger_if_missing {
        CreateMissing::Yes
    } else {
        CreateMissing::No
    };
    let opened = db::open(&config.ledger, create)
        .await
        .map_err(|source| StartupError::ledger_open(&config.ledger, config.mode, source))?;
    let pending = db::pending_migrations(&opened.pool)
        .await
        .map_err(|source| StartupError::MigrationFailed {
            path: config.ledger.path.clone(),
            source: Box::new(source),
        })?;
    if pending > 0 && !opened.created_now {
        let directory = match resolved_backup_directory(config) {
            Ok(directory) => directory,
            Err(error) => {
                opened.pool.close().await;
                return Err(error);
            }
        };
        let path = ledger_path(config)?.to_path_buf();
        if let Err(error) =
            take_snapshot(&opened.pool, &path, directory, SnapshotKind::PreMigration).await
        {
            opened.pool.close().await;
            return Err(StartupError::PreMigrationBackupFailed(error));
        }
    }
    db::migrate(&opened.pool)
        .await
        .map_err(|source| StartupError::MigrationFailed {
            path: config.ledger.path.clone(),
            source: Box::new(source),
        })?;
    let launch_backup = if config.backup_enabled {
        match config.backup_dir.path() {
            Some(directory) => match take_snapshot(
                &opened.pool,
                ledger_path(config)?,
                directory,
                SnapshotKind::Launch,
            )
            .await
            {
                Ok(_) => LaunchBackupOutcome::succeeded(),
                Err(error) => {
                    crate::engine_error!("launch snapshot failed: {error}");
                    LaunchBackupOutcome::failed(error)
                }
            },
            None => {
                let error = format!("resolve backup directory: {}", config.backup_dir.display());
                crate::engine_error!(
                    "launch snapshot failed for ledger {} in {} with snapshot path unavailable during resolve backup directory: {}",
                    ledger_path(config).expect("file-backed ledger").display(),
                    config.backup_dir.display(),
                    error
                );
                LaunchBackupOutcome::failed(error)
            }
        }
    } else {
        LaunchBackupOutcome::disabled()
    };
    if let Some(directory) = config.backup_dir.path() {
        prune(ledger_path(config).expect("file-backed ledger"), directory);
    }
    Ok(OpenedLedgerWithBackup {
        pool: opened.pool,
        created_now: opened.created_now,
        launch_backup,
    })
}

fn ledger_path(config: &AppConfig) -> Result<&std::path::Path, StartupError> {
    config
        .ledger
        .path
        .as_deref()
        .ok_or_else(|| StartupError::LedgerMustBeFileBacked {
            url: config.ledger.url.clone(),
            mode: config.mode,
        })
}

fn resolved_backup_directory(config: &AppConfig) -> Result<&std::path::Path, StartupError> {
    config.backup_dir.path().ok_or_else(|| {
        StartupError::PreMigrationBackupFailed(Box::new(BackupError {
            ledger_path: config.ledger.path.clone().unwrap_or_default(),
            backup_directory: config.backup_dir.display().into(),
            snapshot_path: None,
            operation: "resolve backup directory",
            source: config.backup_dir.display(),
        }))
    })
}

fn prune(ledger_path: &std::path::Path, directory: &std::path::Path) {
    let entries = match std::fs::read_dir(directory) {
        Ok(entries) => entries,
        Err(error) => {
            crate::engine_error!(
                "backup prune failed for ledger {} in {} with snapshot path unavailable during list backup directory: {error}",
                ledger_path.display(),
                directory.display(),
            );
            return;
        }
    };
    let files: Vec<_> = entries
        .filter_map(Result::ok)
        .filter_map(|entry| SnapshotFile::parse(&entry.path()))
        .collect();
    for file in plan_retention(&files, chrono::Utc::now(), &RetentionPolicy::default()).delete {
        if let Err(error) = std::fs::remove_file(&file) {
            crate::engine_error!(
                "backup prune failed for ledger {} in {} for snapshot {} during remove retained snapshot: {error}",
                ledger_path.display(),
                directory.display(),
                file.display()
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        net::{IpAddr, Ipv4Addr},
        path::PathBuf,
        time::{SystemTime, UNIX_EPOCH},
    };

    use crate::{
        config::{AppConfig, Mode},
        ledger::resolve,
    };

    #[tokio::test]
    async fn creates_and_migrates_when_creation_is_explicit() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let directory = PathBuf::from("target/test-ledger-open").join(format!("created-{unique}"));
        fs::create_dir_all(&directory).expect("test directory");
        let path = directory.join("ledger.sqlite");
        let config = AppConfig {
            host: IpAddr::V4(Ipv4Addr::LOCALHOST),
            port: 8480,
            ledger: resolve(
                &format!("sqlite://{}", path.to_string_lossy().replace('\\', "/")),
                Mode::Production,
            )
            .expect("location"),
            static_assets_dir: directory.clone(),
            mode: Mode::Production,
            create_ledger_if_missing: true,
            backup_enabled: false,
            backup_dir: crate::config::BackupDirectory::Resolved(directory.join("backups")),
            log_file: crate::config::LogFile::Unresolved("test".to_owned()),
            market_data_refresh_enabled: false,
            launch_refresh_enabled: false,
        };

        let opened = super::open(&config).await.expect("ledger should open");
        assert!(opened.created_now);
        assert_eq!(
            crate::db::pending_migrations(&opened.pool)
                .await
                .expect("migration status"),
            0
        );
        assert!(!directory.join("backups").exists());
        opened.pool.close().await;
        fs::remove_dir_all(directory).expect("remove test directory");
    }

    #[tokio::test]
    async fn pre_migration_snapshot_is_mandatory_even_when_launch_backups_are_disabled() {
        let directory = PathBuf::from("target/test-ledger-open").join(format!(
            "pending-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&directory).unwrap();
        let path = directory.join("ledger.sqlite");
        let location = resolve(
            &format!("sqlite://{}", path.to_string_lossy().replace('\\', "/")),
            Mode::Production,
        )
        .unwrap();
        let first = crate::db::open(&location, crate::db::CreateMissing::Yes)
            .await
            .unwrap();
        first.pool.close().await;
        let config = AppConfig {
            host: IpAddr::V4(Ipv4Addr::LOCALHOST),
            port: 8480,
            ledger: location,
            static_assets_dir: directory.clone(),
            mode: Mode::Production,
            create_ledger_if_missing: false,
            backup_enabled: false,
            backup_dir: crate::config::BackupDirectory::Resolved(directory.join("backups")),
            log_file: crate::config::LogFile::Unresolved("test".to_owned()),
            market_data_refresh_enabled: false,
            launch_refresh_enabled: false,
        };
        let opened = super::open(&config).await.unwrap();
        assert_eq!(
            opened.launch_backup.status,
            crate::ledger::LaunchBackupStatus::Disabled
        );
        assert!(fs::read_dir(directory.join("backups"))
            .unwrap()
            .any(|entry| entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .contains("premigration")));
        opened.pool.close().await;
        fs::remove_dir_all(directory).unwrap();
    }

    #[tokio::test]
    async fn unresolved_directory_blocks_pending_migration_but_not_launch() {
        let directory = PathBuf::from("target/test-ledger-open").join(format!(
            "unresolved-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&directory).unwrap();
        let path = directory.join("ledger.sqlite");
        let location = resolve(
            &format!("sqlite://{}", path.to_string_lossy().replace('\\', "/")),
            Mode::Production,
        )
        .unwrap();
        let first = crate::db::open(&location, crate::db::CreateMissing::Yes)
            .await
            .unwrap();
        first.pool.close().await;
        let config = AppConfig {
            host: IpAddr::V4(Ipv4Addr::LOCALHOST),
            port: 8480,
            ledger: location,
            static_assets_dir: directory.clone(),
            mode: Mode::Production,
            create_ledger_if_missing: false,
            backup_enabled: true,
            backup_dir: crate::config::BackupDirectory::Unresolved(
                "OneDrive is not set".to_owned(),
            ),
            log_file: crate::config::LogFile::Unresolved("test".to_owned()),
            market_data_refresh_enabled: false,
            launch_refresh_enabled: false,
        };
        let error = super::open(&config)
            .await
            .expect_err("pending migration must require a backup directory");
        assert!(matches!(
            error,
            crate::startup_error::StartupError::PreMigrationBackupFailed(..)
        ));
        fs::remove_dir_all(directory).unwrap();
    }

    #[tokio::test]
    async fn unresolved_directory_is_a_nonfatal_failed_launch_after_migration() {
        let directory = PathBuf::from("target/test-ledger-open").join(format!(
            "launch-failure-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&directory).unwrap();
        let path = directory.join("ledger.sqlite");
        let location = resolve(
            &format!("sqlite://{}", path.to_string_lossy().replace('\\', "/")),
            Mode::Production,
        )
        .unwrap();
        let first = crate::db::open(&location, crate::db::CreateMissing::Yes)
            .await
            .unwrap();
        crate::db::migrate(&first.pool).await.unwrap();
        first.pool.close().await;
        let config = AppConfig {
            host: IpAddr::V4(Ipv4Addr::LOCALHOST),
            port: 8480,
            ledger: location,
            static_assets_dir: directory.clone(),
            mode: Mode::Production,
            create_ledger_if_missing: false,
            backup_enabled: true,
            backup_dir: crate::config::BackupDirectory::Unresolved(
                "OneDrive is not set".to_owned(),
            ),
            log_file: crate::config::LogFile::Unresolved("test".to_owned()),
            market_data_refresh_enabled: false,
            launch_refresh_enabled: false,
        };
        let opened = super::open(&config).await.unwrap();
        assert_eq!(
            opened.launch_backup.status,
            crate::ledger::LaunchBackupStatus::Failed
        );
        assert!(opened.launch_backup.error.is_some());
        opened.pool.close().await;
        fs::remove_dir_all(directory).unwrap();
    }
}
