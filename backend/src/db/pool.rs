use std::str::FromStr;

use sqlx::sqlite::{SqliteConnectOptions, SqlitePool, SqlitePoolOptions};

/// Open the SQLite pool, enabling foreign keys, creating the file if needed,
/// and applying all embedded migrations.
pub async fn connect(database_url: &str) -> Result<SqlitePool, sqlx::Error> {
    let options = SqliteConnectOptions::from_str(database_url)?
        .create_if_missing(true)
        .foreign_keys(true);

    let pool = SqlitePoolOptions::new().connect_with(options).await?;

    sqlx::migrate!("./migrations").run(&pool).await?;

    Ok(pool)
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        path::{Path, PathBuf},
        time::{SystemTime, UNIX_EPOCH},
    };

    use super::connect;

    #[tokio::test]
    async fn connect_creates_file_applies_migrations_and_enables_foreign_keys() {
        let db_path = test_db_path();
        let database_url = format!("sqlite://{}", db_path.display());

        let pool = connect(&database_url)
            .await
            .expect("file database should connect and migrate");

        let table_count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = 'transactions'",
        )
        .fetch_one(&pool)
        .await
        .expect("schema query should succeed");

        assert_eq!(table_count, 1);

        let foreign_keys_enabled: i64 = sqlx::query_scalar("PRAGMA foreign_keys")
            .fetch_one(&pool)
            .await
            .expect("foreign key pragma should be readable");

        assert_eq!(foreign_keys_enabled, 1);

        pool.close().await;

        assert!(db_path.is_file());
        cleanup_sqlite_files(&db_path);
    }

    fn test_db_path() -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time should be after UNIX_EPOCH")
            .as_nanos();
        let directory = PathBuf::from("target").join("test-dbs");
        fs::create_dir_all(&directory).expect("test database directory should be created");
        directory.join(format!("connect-{unique}.sqlite"))
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
