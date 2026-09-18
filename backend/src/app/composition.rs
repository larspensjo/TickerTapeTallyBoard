use std::sync::Arc;

use crate::{
    config::{AppConfig, AssetPolicy},
    startup_error::StartupError,
    state::AppState,
};

pub struct Application {
    pub state: AppState,
    pub router: axum::Router,
    launch_refresh: Option<tokio::task::JoinHandle<()>>,
    runtime: tokio::runtime::Handle,
}

impl Application {
    pub async fn build(
        config: &AppConfig,
        asset_policy: AssetPolicy,
    ) -> Result<Self, StartupError> {
        let state = build_state(config).await?;
        let router = match router_for_assets(config, state.clone(), asset_policy) {
            Ok(router) => router,
            Err(error) => {
                state.pool.close().await;
                return Err(error);
            }
        };
        Ok(Self {
            state,
            router,
            launch_refresh: None,
            runtime: tokio::runtime::Handle::current(),
        })
    }

    /// Callable from any thread: the refresh runs on the runtime that built the
    /// application, so a shell whose main thread lies outside that runtime can start it.
    pub fn start_launch_refresh(&mut self, config: &AppConfig) {
        let _runtime_context = self.runtime.enter();
        self.launch_refresh = spawn_launch_refresh(config, self.state.clone());
    }

    pub async fn shutdown(&mut self) {
        if let Some(handle) = self.launch_refresh.take() {
            handle.abort();
        }
        self.state.pool.close().await;
    }
}

fn router_for_assets(
    config: &AppConfig,
    state: AppState,
    asset_policy: AssetPolicy,
) -> Result<axum::Router, StartupError> {
    match static_assets_available(config.static_assets_dir()) {
        true => {
            crate::engine_info!(
                "serving frontend assets from {}",
                config.static_assets_dir().display()
            );
            Ok(crate::api::router_with_static_assets(
                config.static_assets_dir(),
                state,
            ))
        }
        false if asset_policy == AssetPolicy::Required => Err(StartupError::StaticAssetsMissing {
            dir: config.static_assets_dir.clone(),
        }),
        false => {
            crate::engine_warn!(
                "frontend assets not found at {}; serving backend routes only",
                config.static_assets_dir().display()
            );
            Ok(crate::api::router(state))
        }
    }
}

fn static_assets_available(dir: &std::path::Path) -> bool {
    std::fs::metadata(crate::api::static_assets::index_path(dir))
        .map(|metadata| metadata.is_file() && metadata.len() > 0)
        .unwrap_or(false)
}

async fn build_state(config: &AppConfig) -> Result<AppState, StartupError> {
    let (pool, launch_backup) = if config.mode.is_demo() {
        crate::engine_info!("starting in DEMO mode (in-memory, seeded, read-only)");
        let pool =
            crate::db::memory_pool()
                .await
                .map_err(|source| StartupError::LedgerOpenFailed {
                    path: None,
                    source: Box::new(source),
                })?;
        crate::demo::seed(&pool, crate::clock::Clock::System.today())
            .await
            .map_err(|source| StartupError::LedgerOpenFailed { path: None, source })?;
        sqlx::query("PRAGMA query_only = ON")
            .execute(&pool)
            .await
            .map_err(|source| StartupError::LedgerOpenFailed {
                path: None,
                source: Box::new(source),
            })?;
        (pool, crate::ledger::LaunchBackupOutcome::skipped())
    } else {
        let opened = crate::ledger::open(config).await?;
        (opened.pool, opened.launch_backup)
    };
    Ok(AppState::new(
        pool,
        Arc::new(crate::market_data::MarketDataService::live()),
    )
    .with_mode(config.mode)
    .with_ledger_path(config.ledger.path.clone())
    .with_backup(config.backup_dir.clone(), launch_backup))
}

fn spawn_launch_refresh(
    config: &AppConfig,
    state: AppState,
) -> Option<tokio::task::JoinHandle<()>> {
    if state.is_demo() {
        crate::engine_info!("demo mode active; skipping launch refresh");
        return None;
    }
    if !config.market_data_refresh_enabled || !config.launch_refresh_enabled {
        crate::engine_info!("launch refresh disabled by configuration; skipping startup refresh");
        return None;
    }
    Some(tokio::spawn(async move {
        let request = crate::market_data::RefreshPricesRequest {
            mode: crate::market_data::RefreshMode::Latest,
            start_date: None,
            end_date: None,
        };
        if let Err(error) = state
            .market_data
            .refresh(
                &state.pool,
                state.clock.today(),
                crate::market_data::RefreshTrigger::Launch,
                request,
            )
            .await
        {
            crate::engine_error!("launch refresh failed: {error}");
        }
        state.revision.bump();
        crate::engine_info!(
            "launch refresh finished; data revision is now {}",
            state.revision.current()
        );
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        fs,
        net::{IpAddr, Ipv4Addr},
        path::PathBuf,
        sync::Arc,
        time::{SystemTime, UNIX_EPOCH},
    };

    use axum::body::to_bytes;
    use axum::http::{Request, StatusCode};
    use chrono::NaiveDate;
    use rust_decimal_macros::dec;
    use tokio::sync::Notify;
    use tower::ServiceExt;

    use crate::{
        config::{BackupDirectory, LogFile, Mode},
        db::{self, instruments, provider_symbols, transactions},
        ledger::{memory, resolve, LedgerLocation},
        market_data::MarketDataService,
        providers::{
            DailyClose, FakeFxRateProvider, FakePriceProvider, FxProvider, FxRate,
            MarketDataProvider,
        },
        state::AppState,
    };

    fn test_config(mode: Mode, ledger: LedgerLocation) -> AppConfig {
        AppConfig {
            host: IpAddr::V4(Ipv4Addr::LOCALHOST),
            port: 8480,
            ledger,
            static_assets_dir: unique_assets_dir("test-assets"),
            mode,
            create_ledger_if_missing: false,
            backup_enabled: false,
            backup_dir: BackupDirectory::Unresolved("test".to_owned()),
            log_file: LogFile::Unresolved("test".to_owned()),
            log_max_bytes: crate::engine_logging::DEFAULT_MAX_BYTES,
            market_data_refresh_enabled: true,
            launch_refresh_enabled: true,
        }
    }

    async fn seeded_state() -> (AppState, FakePriceProvider, Arc<Notify>) {
        let pool = db::memory_pool().await.expect("memory pool");
        let price_provider = FakePriceProvider::with_provider(MarketDataProvider::Yahoo);
        let fx_provider = FakeFxRateProvider::with_provider(FxProvider::Frankfurter);
        let gate = Arc::new(Notify::new());

        price_provider.block_next_call_on(Arc::clone(&gate));
        price_provider.push_response(Ok(vec![DailyClose {
            provider: MarketDataProvider::Yahoo,
            provider_symbol: "MSFT".to_owned(),
            date: NaiveDate::from_ymd_opt(2026, 6, 12).expect("date"),
            close: dec!(101),
            currency: "USD".to_owned(),
        }]));
        fx_provider.push_response(Ok(vec![FxRate {
            provider: FxProvider::Frankfurter,
            base: "USD".to_owned(),
            quote: "SEK".to_owned(),
            date: NaiveDate::from_ymd_opt(2026, 6, 12).expect("date"),
            rate: dec!(10.5),
        }]));

        let state = AppState::with_market_data(
            pool,
            MarketDataService::with_providers(price_provider.clone(), fx_provider),
        );
        let (instrument, _) = instruments::upsert(
            &state.pool,
            &crate::db::instruments::NewInstrument {
                symbol: "MSFT".to_owned(),
                exchange: "NASDAQ".to_owned(),
                name: "Microsoft".to_owned(),
                kind: "STOCK".to_owned(),
                currency: "USD".to_owned(),
                isin: None,
            },
        )
        .await
        .expect("instrument upsert should succeed");
        transactions::insert(
            &state.pool,
            &crate::db::transactions::NewTransaction {
                instrument_id: instrument.id,
                kind: crate::domain::TransactionKind::Buy,
                trade_date: NaiveDate::from_ymd_opt(2026, 6, 10).expect("date"),
                quantity: 10,
                price: Some(dec!(100)),
                dividend_per_share: None,
                currency: Some("USD".to_owned()),
                fx_rate_to_base: Some(dec!(10)),
                brokerage: None,
                note: None,
            },
        )
        .await
        .expect("transaction insert should succeed");

        provider_symbols::upsert(
            &state.pool,
            &crate::db::provider_symbols::NewProviderSymbol {
                instrument_id: instrument.id,
                provider: MarketDataProvider::Yahoo,
                provider_symbol: "MSFT".to_owned(),
                asset_class: None,
                currency: Some("USD".to_owned()),
                enabled: true,
                created_at: crate::import::now_iso8601(),
                updated_at: crate::import::now_iso8601(),
            },
        )
        .await
        .expect("provider symbol upsert should succeed");

        (state, price_provider, gate)
    }

    #[tokio::test]
    async fn launch_refresh_spawns_background_job() {
        let (state, price_provider, gate) = seeded_state().await;
        let config = test_config(Mode::Production, memory());

        let handle = spawn_launch_refresh(&config, state.clone())
            .expect("launch refresh should be scheduled");

        while price_provider.calls().is_empty() {
            tokio::task::yield_now().await;
        }

        let status = state
            .market_data
            .status(&state.pool, state.clock.today())
            .await
            .expect("status should succeed");
        assert!(status.refreshing);
        assert_eq!(
            status.latest_run.expect("latest run").trigger,
            crate::market_data::RefreshTrigger::Launch
        );

        gate.notify_waiters();
        let before = state.revision.current();
        handle.await.expect("launch task should finish");
        assert_ne!(state.revision.current(), before);

        let status = state
            .market_data
            .status(&state.pool, state.clock.today())
            .await
            .expect("status should succeed");
        assert!(!status.refreshing);
        assert_eq!(
            status.latest_run.expect("latest run").status,
            crate::market_data::RefreshRunStatus::Succeeded
        );
    }

    #[tokio::test]
    async fn launch_refresh_is_skipped_when_disabled() {
        let (state, _, _) = seeded_state().await;
        let mut config = test_config(Mode::Production, memory());
        config.launch_refresh_enabled = false;

        assert!(spawn_launch_refresh(&config, state).is_none());
    }

    #[tokio::test]
    async fn launch_refresh_is_skipped_in_demo_mode() {
        let (state, _, _) = seeded_state().await;
        let state = state.with_mode(Mode::Demo);
        let config = test_config(Mode::Demo, memory());

        assert!(spawn_launch_refresh(&config, state).is_none());
    }

    #[tokio::test]
    async fn demo_state_is_seeded_and_query_only() {
        let backup_directory = unique_assets_dir("demo-backups");
        let mut config = test_config(Mode::Demo, memory());
        config.backup_enabled = true;
        config.backup_dir = BackupDirectory::Resolved(backup_directory.clone());

        let state = build_state(&config).await.expect("demo state should build");

        assert!(state.is_demo());
        assert_eq!(
            state.backup.launch.status,
            crate::ledger::LaunchBackupStatus::Skipped
        );
        assert!(!backup_directory.exists());
        let instruments = db::instruments::list(&state.pool)
            .await
            .expect("seeded instruments should list");
        assert_eq!(instruments.len(), 7);

        let write_result = sqlx::query(
            "INSERT INTO instruments (symbol, exchange, name, type, currency, isin) \
             VALUES ('FAIL', 'TEST', 'Should Fail', 'STOCK', 'SEK', NULL)",
        )
        .execute(&state.pool)
        .await;

        assert!(write_result.is_err());
        state.pool.close().await;
    }

    #[test]
    fn asset_availability_requires_non_empty_index() {
        let directory = unique_assets_dir("availability");
        fs::create_dir_all(&directory).expect("test assets directory should be created");

        assert!(!static_assets_available(&directory));
        fs::write(crate::api::static_assets::index_path(&directory), "ok")
            .expect("test index should be written");
        assert!(static_assets_available(&directory));

        fs::remove_dir_all(directory).expect("test assets directory should be removed");
    }

    #[tokio::test]
    async fn required_assets_reject_an_empty_directory() {
        let directory = unique_assets_dir("required");
        fs::create_dir_all(&directory).expect("test assets directory should be created");
        let mut config = test_config(Mode::Production, memory());
        config.static_assets_dir = directory.clone();
        let state = AppState::for_tests().await;

        let result = router_for_assets(&config, state, AssetPolicy::Required);

        assert!(matches!(
            result,
            Err(StartupError::StaticAssetsMissing { .. })
        ));
        fs::remove_dir_all(directory).expect("test assets directory should be removed");
    }

    fn unique_assets_dir(name: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time should be after UNIX_EPOCH")
            .as_nanos();
        crate::test_support::workspace_target_path("test-assets").join(format!("{name}-{unique}"))
    }

    fn file_config(mode: Mode, root: &std::path::Path, assets: PathBuf) -> AppConfig {
        let ledger_path = root.join("ledger.sqlite");
        AppConfig {
            host: IpAddr::V4(Ipv4Addr::LOCALHOST),
            port: 8480,
            ledger: resolve(
                &format!(
                    "sqlite://{}",
                    ledger_path.to_string_lossy().replace('\\', "/")
                ),
                mode,
            )
            .expect("ledger location"),
            static_assets_dir: assets,
            mode,
            create_ledger_if_missing: true,
            backup_enabled: false,
            backup_dir: BackupDirectory::Unresolved("test".to_owned()),
            log_file: LogFile::Unresolved("test".to_owned()),
            log_max_bytes: crate::engine_logging::DEFAULT_MAX_BYTES,
            market_data_refresh_enabled: false,
            launch_refresh_enabled: false,
        }
    }

    async fn assert_asset_case(mode: Mode, desktop: bool, index: Option<&str>) {
        let root = unique_assets_dir("application-assets");
        let assets = root.join("assets");
        fs::create_dir_all(&assets).expect("asset directory");
        if let Some(contents) = index {
            fs::write(crate::api::static_assets::index_path(&assets), contents)
                .expect("index file");
        }
        let mut config = if mode.is_demo() {
            test_config(mode, memory())
        } else {
            file_config(mode, &root, assets.clone())
        };
        config.static_assets_dir = assets;
        let policy = if desktop {
            AssetPolicy::Required
        } else {
            mode.asset_policy()
        };
        let application = Application::build(&config, policy).await;
        let should_build = index.is_some_and(|contents| !contents.is_empty());
        if should_build {
            let mut application = application.expect("valid assets should build");
            let response = application
                .router
                .clone()
                .oneshot(
                    Request::builder()
                        .uri("/")
                        .body(axum::body::Body::empty())
                        .unwrap(),
                )
                .await
                .expect("SPA request should complete");
            assert_eq!(response.status(), StatusCode::OK);
            application.shutdown().await;
        } else if policy == AssetPolicy::Required {
            assert!(matches!(
                application,
                Err(StartupError::StaticAssetsMissing { .. })
            ));
        } else {
            let mut application = application.expect("optional assets should build");
            let health = application
                .router
                .clone()
                .oneshot(
                    Request::builder()
                        .uri("/api/health")
                        .body(axum::body::Body::empty())
                        .unwrap(),
                )
                .await
                .expect("health request should complete");
            assert_eq!(health.status(), StatusCode::OK);
            let response = application
                .router
                .clone()
                .oneshot(
                    Request::builder()
                        .uri("/app-route")
                        .body(axum::body::Body::empty())
                        .unwrap(),
                )
                .await
                .expect("API-only root request should complete");
            assert_eq!(response.status(), StatusCode::NOT_FOUND);
            application.shutdown().await;
        }
        let _ = fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn application_asset_matrix_covers_web_and_desktop_modes() {
        for mode in [Mode::Production, Mode::Development, Mode::Demo] {
            for desktop in [false, true] {
                for index in [None, Some(""), Some("<html>app</html>")] {
                    assert_asset_case(mode, desktop, index).await;
                }
            }
        }
    }

    #[tokio::test]
    async fn application_build_does_not_start_launch_refresh() {
        let root = unique_assets_dir("build-without-refresh");
        let mut config = file_config(Mode::Production, &root, root.join("assets"));
        config.market_data_refresh_enabled = true;
        config.launch_refresh_enabled = true;

        let mut application = Application::build(&config, AssetPolicy::Optional)
            .await
            .expect("application should build without starting a refresh");

        assert!(application.launch_refresh.is_none());
        assert!(crate::db::market_data_runs::latest(&application.state.pool)
            .await
            .expect("latest refresh run should load")
            .is_none());
        application.shutdown().await;
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn launch_refresh_starts_from_a_thread_outside_the_runtime() {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .expect("runtime should build");
        let root = unique_assets_dir("refresh-outside-runtime");
        let mut config = file_config(Mode::Production, &root, root.join("assets"));
        config.market_data_refresh_enabled = true;
        config.launch_refresh_enabled = true;
        let mut application = runtime
            .block_on(Application::build(&config, AssetPolicy::Optional))
            .expect("application should build");

        application.start_launch_refresh(&config);

        assert!(application.launch_refresh.is_some());
        runtime.block_on(application.shutdown());
        let _ = fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn application_serves_health_and_spa_and_shutdown_closes_pool() {
        let root = unique_assets_dir("application-contract");
        let assets = root.join("assets");
        fs::create_dir_all(&assets).expect("asset directory");
        fs::write(
            crate::api::static_assets::index_path(&assets),
            "<html>app</html>",
        )
        .expect("index file");
        let mut config = test_config(Mode::Demo, memory());
        config.static_assets_dir = assets;
        let mut application = Application::build(&config, AssetPolicy::Required)
            .await
            .expect("demo application should build");
        let pool = application.state.pool.clone();

        let health = application
            .router
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/health")
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .expect("health request should complete");
        assert_eq!(health.status(), StatusCode::OK);
        let index = application
            .router
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/")
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .expect("index request should complete");
        assert_eq!(index.status(), StatusCode::OK);
        assert_eq!(
            to_bytes(index.into_body(), usize::MAX).await.unwrap(),
            "<html>app</html>"
        );

        application.shutdown().await;
        application.shutdown().await;
        assert!(sqlx::query("SELECT 1").execute(&pool).await.is_err());
        let _ = fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn demo_application_uses_no_ledger_filesystem_path() {
        let mut config = test_config(Mode::Demo, memory());
        config.static_assets_dir = unique_assets_dir("demo-no-ledger");
        let mut application = Application::build(&config, AssetPolicy::Optional)
            .await
            .expect("demo application should build without a ledger");
        assert!(application.state.ledger_path.is_none());
        application.shutdown().await;
        assert!(!config.static_assets_dir.exists());
    }
}
