use std::{fmt, path::PathBuf};

use crate::{
    config::{ConfigError, Mode},
    db::OpenError,
    ledger::{BackupError, LedgerLocation, LedgerLocationError},
};

#[derive(Debug)]
pub enum StartupError {
    Config(ConfigError),
    LedgerMissing {
        path: PathBuf,
    },
    LedgerMustBeFileBacked {
        url: String,
        mode: Mode,
    },
    LedgerNotAbsolute {
        url: String,
    },
    LedgerNotAFile {
        path: PathBuf,
    },
    LedgerUnsupportedUrl {
        url: String,
    },
    LedgerOpenFailed {
        path: Option<PathBuf>,
        source: Box<dyn std::error::Error + Send + Sync>,
    },
    MigrationFailed {
        path: Option<PathBuf>,
        source: Box<dyn std::error::Error + Send + Sync>,
    },
    PreMigrationBackupFailed(Box<BackupError>),
    StaticAssetsMissing {
        dir: PathBuf,
    },
    PortUnavailable {
        address: String,
        source: std::io::Error,
    },
}

impl StartupError {
    pub fn ledger_open(location: &LedgerLocation, mode: Mode, source: OpenError) -> Self {
        match source {
            OpenError::Missing { path } => Self::LedgerMissing { path },
            OpenError::NotFileBacked => Self::LedgerMustBeFileBacked {
                url: location.url.clone(),
                mode,
            },
            OpenError::Sqlx(source) => Self::LedgerOpenFailed {
                path: location.path.clone(),
                source: Box::new(source),
            },
        }
    }
}

impl From<ConfigError> for StartupError {
    fn from(error: ConfigError) -> Self {
        match error.into_location() {
            Ok(error) => error.into(),
            Err(error) => Self::Config(error),
        }
    }
}

impl From<LedgerLocationError> for StartupError {
    fn from(error: LedgerLocationError) -> Self {
        match error {
            LedgerLocationError::MustBeFileBacked { url, mode } => {
                Self::LedgerMustBeFileBacked { url, mode }
            }
            LedgerLocationError::UnsupportedUrl { url } => Self::LedgerUnsupportedUrl { url },
            LedgerLocationError::NotAbsolute { url } => Self::LedgerNotAbsolute { url },
            LedgerLocationError::NotAFile { path } => Self::LedgerNotAFile { path },
        }
    }
}

impl fmt::Display for StartupError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Config(error) => write!(f, "{error}"),
            Self::LedgerMissing { path } => write!(
                f,
                "ledger is missing at {}; initialize it with scripts/start.ps1 -InitLedger",
                path.display()
            ),
            Self::LedgerMustBeFileBacked { url, mode } => write!(
                f,
                "{}",
                LedgerLocationError::MustBeFileBacked {
                    url: url.clone(),
                    mode: *mode,
                }
            ),
            Self::LedgerNotAFile { path } => write!(
                f,
                "{}",
                LedgerLocationError::NotAFile { path: path.clone() }
            ),
            Self::LedgerNotAbsolute { url } => write!(
                f,
                "{}",
                LedgerLocationError::NotAbsolute { url: url.clone() }
            ),
            Self::LedgerUnsupportedUrl { url } => write!(
                f,
                "{}",
                LedgerLocationError::UnsupportedUrl { url: url.clone() }
            ),
            Self::LedgerOpenFailed { path, source } => write!(
                f,
                "failed to open ledger {}: {source}",
                path.as_ref()
                    .map_or_else(|| "in-memory".to_owned(), |path| path.display().to_string())
            ),
            Self::MigrationFailed { path, source } => write!(
                f,
                "failed to migrate ledger {}: {source}",
                path.as_ref()
                    .map_or_else(|| "in-memory".to_owned(), |path| path.display().to_string())
            ),
            Self::PreMigrationBackupFailed(failure) => {
                write!(f, "mandatory pre-migration backup failed: {failure}")
            }
            Self::StaticAssetsMissing { dir } => write!(
                f,
                "built frontend assets are missing from {}; run npm run build",
                dir.display()
            ),
            Self::PortUnavailable { address, source } => write!(
                f,
                "port unavailable at {address}; choose TTTB_PORT: {source}"
            ),
        }
    }
}

impl std::error::Error for StartupError {}

#[cfg(test)]
mod tests {
    use super::StartupError;
    use crate::{
        config::{ConfigError, Mode},
        ledger::BackupError,
    };
    use std::path::PathBuf;

    #[test]
    fn display_preserves_the_identifying_value_for_every_variant() {
        let cases = vec![
            (
                StartupError::Config(ConfigError::value(
                    "TTTB_TEST",
                    "configured-value".to_owned(),
                    "test failure",
                )),
                "configured-value",
            ),
            (
                StartupError::LedgerMissing {
                    path: PathBuf::from("C:/ledgers/missing.sqlite"),
                },
                "C:/ledgers/missing.sqlite",
            ),
            (
                StartupError::LedgerMustBeFileBacked {
                    url: "sqlite::memory:".to_owned(),
                    mode: Mode::Production,
                },
                "sqlite::memory:",
            ),
            (
                StartupError::LedgerNotAbsolute {
                    url: "sqlite://relative.sqlite".to_owned(),
                },
                "sqlite://relative.sqlite",
            ),
            (
                StartupError::LedgerNotAFile {
                    path: PathBuf::from("C:/ledgers/directory"),
                },
                "C:/ledgers/directory",
            ),
            (
                StartupError::LedgerUnsupportedUrl {
                    url: "postgres://ledger".to_owned(),
                },
                "postgres://ledger",
            ),
            (
                StartupError::LedgerOpenFailed {
                    path: Some(PathBuf::from("C:/ledgers/open.sqlite")),
                    source: Box::new(std::io::Error::other("open failure")),
                },
                "C:/ledgers/open.sqlite",
            ),
            (
                StartupError::MigrationFailed {
                    path: Some(PathBuf::from("C:/ledgers/migrate.sqlite")),
                    source: Box::new(std::io::Error::other("migration failure")),
                },
                "C:/ledgers/migrate.sqlite",
            ),
            (
                StartupError::PreMigrationBackupFailed(Box::new(BackupError {
                    ledger_path: PathBuf::from("C:/ledgers/backup.sqlite"),
                    backup_directory: PathBuf::from("D:/backups"),
                    snapshot_path: None,
                    operation: "test",
                    source: "backup failure".to_owned(),
                })),
                "C:/ledgers/backup.sqlite",
            ),
            (
                StartupError::StaticAssetsMissing {
                    dir: PathBuf::from("C:/source/frontend/dist"),
                },
                "C:/source/frontend/dist",
            ),
            (
                StartupError::PortUnavailable {
                    address: "127.0.0.1:8480".to_owned(),
                    source: std::io::Error::new(std::io::ErrorKind::AddrInUse, "already in use"),
                },
                "127.0.0.1:8480",
            ),
        ];

        for (error, identifying_value) in cases {
            assert!(
                error.to_string().contains(identifying_value),
                "{error:?} must retain {identifying_value:?}"
            );
        }
    }
}
