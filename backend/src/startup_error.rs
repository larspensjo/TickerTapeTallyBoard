use std::{fmt, path::PathBuf};

use crate::{
    config::{ConfigError, Mode},
    db::OpenError,
    ledger::{LedgerLocation, LedgerLocationError},
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
