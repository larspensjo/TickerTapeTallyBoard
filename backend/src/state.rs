use sqlx::sqlite::SqlitePool;

/// Shared application state injected into axum handlers via `State`.
#[derive(Clone)]
pub struct AppState {
    #[allow(dead_code)]
    pub pool: SqlitePool,
}

impl AppState {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }
}

#[cfg(test)]
impl AppState {
    /// Build state backed by a migrated in-memory database for tests.
    pub async fn for_tests() -> Self {
        Self::new(crate::db::testing::memory_pool().await)
    }
}
