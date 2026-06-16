use std::sync::Arc;

use sqlx::sqlite::SqlitePool;

use crate::market_data::MarketDataService;

/// Shared application state injected into axum handlers via `State`.
#[derive(Clone)]
pub struct AppState {
    pub pool: SqlitePool,
    pub market_data: Arc<MarketDataService>,
}

impl AppState {
    pub fn new(pool: SqlitePool, market_data: Arc<MarketDataService>) -> Self {
        Self { pool, market_data }
    }

    pub fn with_market_data(pool: SqlitePool, market_data: MarketDataService) -> Self {
        Self::new(pool, Arc::new(market_data))
    }

    /// Build state backed by a migrated in-memory database for tests.
    pub async fn for_tests() -> Self {
        Self::with_market_data(
            crate::db::testing::memory_pool().await,
            MarketDataService::with_providers(
                crate::providers::FakePriceProvider::new(),
                crate::providers::FakeFxRateProvider::new(),
            ),
        )
    }
}
