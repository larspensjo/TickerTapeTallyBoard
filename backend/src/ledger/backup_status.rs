use super::{SnapshotFile, SnapshotKind};
use chrono::{DateTime, Utc};
use std::{fs, path::Path};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BackupDirectoryStatus {
    pub last_snapshot_at: Option<DateTime<Utc>>,
    pub snapshot_count: usize,
    pub listing_error: Option<String>,
}

pub fn backup_status(directory: &Path) -> BackupDirectoryStatus {
    let entries = match fs::read_dir(directory) {
        Ok(entries) => entries,
        Err(error) => {
            return BackupDirectoryStatus {
                last_snapshot_at: None,
                snapshot_count: 0,
                listing_error: Some(error.to_string()),
            }
        }
    };
    let snapshots: Vec<_> = entries
        .filter_map(Result::ok)
        .filter_map(|entry| SnapshotFile::parse(&entry.path()))
        .filter(|file| matches!(file.kind, SnapshotKind::Launch | SnapshotKind::PreMigration))
        .collect();
    BackupDirectoryStatus {
        last_snapshot_at: snapshots.iter().map(|file| file.timestamp).max(),
        snapshot_count: snapshots.len(),
        listing_error: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        path::PathBuf,
        time::{SystemTime, UNIX_EPOCH},
    };

    fn temp_directory(name: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        PathBuf::from("target/test-backup-status").join(format!("{name}-{unique}"))
    }

    #[test]
    fn missing_directory_is_explicit() {
        let directory = temp_directory("missing");
        let status = backup_status(&directory);

        assert_eq!(status.snapshot_count, 0);
        assert_eq!(status.last_snapshot_at, None);
        assert!(status.listing_error.is_some());
    }

    #[test]
    fn empty_directory_has_no_snapshots_and_no_listing_error() {
        let directory = temp_directory("empty");
        fs::create_dir_all(&directory).expect("test directory");

        let status = backup_status(&directory);

        assert_eq!(status.last_snapshot_at, None);
        assert_eq!(status.snapshot_count, 0);
        assert_eq!(status.listing_error, None);
        fs::remove_dir_all(directory).expect("remove test directory");
    }

    #[test]
    fn partial_and_failed_files_are_never_counted_as_backups() {
        let directory = temp_directory("ignored-artifacts");
        fs::create_dir_all(&directory).expect("test directory");
        for name in [
            "portfolio-20260829T143012457Z.sqlite.partial",
            "portfolio-20260829T143012457Z.sqlite.failed",
            "portfolio-20260829T143012457Z.sqlite",
        ] {
            fs::write(directory.join(name), []).expect("test artifact");
        }

        let status = backup_status(&directory);

        assert_eq!(status.snapshot_count, 1);
        assert_eq!(
            status.last_snapshot_at,
            Some(
                DateTime::parse_from_rfc3339("2026-08-29T14:30:12.457Z")
                    .expect("timestamp")
                    .with_timezone(&Utc)
            )
        );
        assert_eq!(status.listing_error, None);
        fs::remove_dir_all(directory).expect("remove test directory");
    }
}
