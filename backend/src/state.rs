use std::{path::PathBuf, sync::Arc};

use sqlx::sqlite::SqlitePool;

use crate::{
    config::{BackupDirectory, Mode},
    ledger::{LaunchBackupOutcome, LaunchBackupStatus},
    market_data::MarketDataService,
};

#[derive(Clone)]
pub struct BackupState {
    pub directory: BackupDirectory,
    pub launch: LaunchBackupOutcome,
}

/// Shared application state injected into axum handlers via `State`.
#[derive(Clone)]
pub struct AppState {
    pub pool: SqlitePool,
    pub market_data: Arc<MarketDataService>,
    pub mode: Mode,
    pub ledger_path: Option<PathBuf>,
    pub backup: BackupState,
}

impl AppState {
    pub fn new(pool: SqlitePool, market_data: Arc<MarketDataService>) -> Self {
        Self {
            pool,
            market_data,
            mode: Mode::Production,
            ledger_path: None,
            backup: BackupState {
                directory: BackupDirectory::Unresolved("not configured".to_owned()),
                launch: LaunchBackupOutcome {
                    status: LaunchBackupStatus::Skipped,
                    error: None,
                },
            },
        }
    }

    pub fn with_mode(mut self, mode: Mode) -> Self {
        self.mode = mode;
        self
    }

    pub fn with_ledger_path(mut self, path: Option<PathBuf>) -> Self {
        self.ledger_path = path;
        self
    }

    pub fn with_backup(mut self, directory: BackupDirectory, launch: LaunchBackupOutcome) -> Self {
        self.backup = BackupState { directory, launch };
        self
    }

    pub fn is_demo(&self) -> bool {
        self.mode.is_demo()
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
