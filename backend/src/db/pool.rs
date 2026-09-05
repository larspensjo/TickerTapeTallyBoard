use std::{fmt, fs, path::PathBuf, str::FromStr};

use sqlx::{
    sqlite::{SqliteConnectOptions, SqlitePool, SqlitePoolOptions},
    Row,
};

use crate::ledger::LedgerLocation;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CreateMissing {
    Yes,
    No,
}

pub struct OpenedLedger {
    pub pool: SqlitePool,
    pub created_now: bool,
}

#[derive(Debug)]
pub enum OpenError {
    Missing { path: PathBuf },
    NotFileBacked,
    Sqlx(sqlx::Error),
}

impl fmt::Display for OpenError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Missing { path } => {
                write!(formatter, "ledger does not exist: {}", path.display())
            }
            Self::NotFileBacked => write!(formatter, "ledger location is not file-backed"),
            Self::Sqlx(error) => write!(formatter, "{error}"),
        }
    }
}

impl std::error::Error for OpenError {}

impl From<sqlx::Error> for OpenError {
    fn from(error: sqlx::Error) -> Self {
        Self::Sqlx(error)
    }
}

pub async fn open(
    location: &LedgerLocation,
    create: CreateMissing,
) -> Result<OpenedLedger, OpenError> {
    let path = location.path.as_ref().ok_or(OpenError::NotFileBacked)?;
    let existed = path.exists();

    if !existed && matches!(create, CreateMissing::No) {
        return Err(OpenError::Missing { path: path.clone() });
    }

    if !existed && matches!(create, CreateMissing::Yes) {
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                fs::create_dir_all(parent)
                    .map_err(|error| OpenError::Sqlx(sqlx::Error::Io(error)))?;
            }
        }
    }

    let options = SqliteConnectOptions::from_str(&location.url)?
        .create_if_missing(matches!(create, CreateMissing::Yes))
        .foreign_keys(true);
    let pool = SqlitePoolOptions::new().connect_with(options).await?;

    Ok(OpenedLedger {
        pool,
        created_now: !existed,
    })
}

pub async fn pending_migrations(pool: &SqlitePool) -> Result<usize, sqlx::Error> {
    let applied = match sqlx::query("SELECT version FROM _sqlx_migrations WHERE success = 1")
        .fetch_all(pool)
        .await
    {
        Ok(rows) => rows
            .into_iter()
            .map(|row| row.get::<i64, _>("version"))
            .collect::<Vec<_>>(),
        Err(sqlx::Error::Database(error)) if error.message().contains("no such table") => {
            Vec::new()
        }
        Err(error) => return Err(error),
    };
    Ok(sqlx::migrate!("./migrations")
        .iter()
        .filter(|migration| !applied.contains(&migration.version))
        .count())
}

pub async fn migrate(pool: &SqlitePool) -> Result<(), sqlx::Error> {
    sqlx::migrate!("./migrations")
        .run(pool)
        .await
        .map_err(|error| sqlx::Error::Configuration(Box::new(error)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{config::Mode, ledger::resolve};
    use std::{
        fs,
        path::{Path, PathBuf},
        time::{SystemTime, UNIX_EPOCH},
    };

    fn test_db_path(name: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time should be after UNIX_EPOCH")
            .as_nanos();
        let directory = PathBuf::from("target").join("test-dbs");
        fs::create_dir_all(&directory).expect("test database directory should be created");
        directory.join(format!("{name}-{unique}.sqlite"))
    }

    fn location_for(path: &Path) -> LedgerLocation {
        resolve(
            &format!("sqlite://{}", path.to_string_lossy().replace('\\', "/")),
            Mode::Production,
        )
        .expect("test ledger location should resolve")
    }

    #[tokio::test]
    async fn no_create_leaves_missing_path_untouched() {
        let path = test_db_path("missing");
        let result = open(&location_for(&path), CreateMissing::No).await;

        assert!(matches!(result, Err(OpenError::Missing { .. })));
        assert!(!path.exists());
    }

    #[tokio::test]
    async fn open_creates_file_reports_pending_migrations_and_enables_foreign_keys() {
        let path = test_db_path("created");
        let location = location_for(&path);

        let opened = open(&location, CreateMissing::Yes)
            .await
            .expect("file database should open");

        assert!(opened.created_now);

        let foreign_keys_enabled: i64 = sqlx::query_scalar("PRAGMA foreign_keys")
            .fetch_one(&opened.pool)
            .await
            .expect("foreign key pragma should be readable");
        assert_eq!(foreign_keys_enabled, 1);

        opened.pool.close().await;

        let opened = open(&location, CreateMissing::No)
            .await
            .expect("existing database should open");
        assert!(!opened.created_now);
        assert_eq!(
            pending_migrations(&opened.pool)
                .await
                .expect("pending migrations should be reported"),
            sqlx::migrate!("./migrations").iter().count()
        );

        migrate(&opened.pool)
            .await
            .expect("migrations should succeed");
        assert_eq!(
            pending_migrations(&opened.pool)
                .await
                .expect("migration status should be reported"),
            0
        );
        opened.pool.close().await;

        assert!(path.is_file());
        cleanup_sqlite_files(&path);
    }

    #[tokio::test]
    async fn create_missing_creates_parent_directories() {
        let path = test_db_path("missing-parent-root")
            .with_extension("")
            .join("nested")
            .join("created.sqlite");
        let parent = path.parent().expect("ledger should have a parent");
        assert!(!parent.exists());

        let opened = open(&location_for(&path), CreateMissing::Yes)
            .await
            .expect("file database and parents should be created");

        assert!(opened.created_now);
        assert!(path.is_file());
        opened.pool.close().await;
        cleanup_sqlite_files(&path);
        fs::remove_dir_all(
            path.parent()
                .and_then(Path::parent)
                .expect("test root should exist"),
        )
        .expect("test directories should be removed");
    }

    #[tokio::test]
    async fn memory_location_returns_typed_error() {
        let result = open(&crate::ledger::memory(), CreateMissing::Yes).await;

        assert!(matches!(result, Err(OpenError::NotFileBacked)));
    }

    fn cleanup_sqlite_files(db_path: &Path) {
        for path in [
            db_path.to_path_buf(),
            db_path.with_extension("sqlite-shm"),
            db_path.with_extension("sqlite-wal"),
        ] {
            match fs::remove_file(&path) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => panic!("failed to remove {}: {error}", path.display()),
            }
        }
    }
}
