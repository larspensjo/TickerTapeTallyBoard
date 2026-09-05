use std::{fmt, path::PathBuf, str::FromStr};

use sqlx::sqlite::SqliteConnectOptions;

use crate::mode::Mode;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LedgerLocation {
    pub url: String,
    pub path: Option<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LedgerLocationError {
    MustBeFileBacked { url: String, mode: Mode },
    UnsupportedUrl { url: String },
    NotAFile { path: PathBuf },
}
impl fmt::Display for LedgerLocationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MustBeFileBacked { url, mode } => {
                write!(
                    f,
                    "ledger {url} must be file-backed in {} mode",
                    mode.as_str()
                )
            }
            Self::UnsupportedUrl { url } => write!(f, "unsupported ledger URL: {url}"),
            Self::NotAFile { path } => {
                write!(f, "ledger path is not a file: {}", path.display())
            }
        }
    }
}

impl std::error::Error for LedgerLocationError {}

pub fn memory() -> LedgerLocation {
    LedgerLocation {
        url: "sqlite::memory:".to_owned(),
        path: None,
    }
}

pub fn resolve(url: &str, mode: Mode) -> Result<LedgerLocation, LedgerLocationError> {
    if mode.is_demo() {
        return Ok(memory());
    }
    if !url.starts_with("sqlite:") {
        return Err(LedgerLocationError::UnsupportedUrl {
            url: url.to_owned(),
        });
    }
    let options =
        SqliteConnectOptions::from_str(url).map_err(|_| LedgerLocationError::UnsupportedUrl {
            url: url.to_owned(),
        })?;
    let path = options.get_filename().to_path_buf();
    if is_memory_url(url) {
        return Err(LedgerLocationError::MustBeFileBacked {
            url: url.to_owned(),
            mode,
        });
    }
    if path.is_dir() {
        return Err(LedgerLocationError::NotAFile { path });
    }
    Ok(LedgerLocation {
        url: url.to_owned(),
        path: Some(path),
    })
}

fn is_memory_url(url: &str) -> bool {
    // sqlx parses this into an internal in-memory flag but does not expose a
    // getter, so keep the supported URL spellings centralised here.
    url.contains(":memory:") || url.contains("mode=memory")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_and_memory_contract() {
        let location = resolve("sqlite://C:/temp/ledger.sqlite", Mode::Production).unwrap();
        assert!(location.path.unwrap().is_absolute());
        assert!(matches!(
            resolve("postgres://x", Mode::Production),
            Err(LedgerLocationError::UnsupportedUrl { .. })
        ));
        assert!(matches!(
            resolve("sqlite::memory:", Mode::Production),
            Err(LedgerLocationError::MustBeFileBacked { .. })
        ));
        assert!(resolve("sqlite::memory:", Mode::Demo)
            .unwrap()
            .path
            .is_none());
    }

    #[test]
    fn recognises_supported_memory_url_spellings() {
        for url in [
            "sqlite::memory:",
            "sqlite://:memory:",
            "sqlite://ignored.sqlite?mode=memory",
        ] {
            assert!(is_memory_url(url), "{url}");
            assert!(matches!(
                resolve(url, Mode::Production),
                Err(LedgerLocationError::MustBeFileBacked { .. })
            ));
        }
    }
}
