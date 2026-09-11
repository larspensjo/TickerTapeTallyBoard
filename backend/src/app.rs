use std::sync::Arc;

use crate::{
    config::{AppConfig, AssetPolicy, LogFile},
    engine_logging::{LogInitOutcome, LogSettings},
    startup_error::StartupError,
    state::AppState,
};

pub async fn run() -> Result<(), StartupError> {
    match run_inner().await {
        Ok(()) => Ok(()),
        Err(failure) => {
            if failure.logging_initialized {
                crate::engine_error!("startup failed: {}", failure.error);
            } else {
                eprintln!("startup failed: {}", failure.error);
            }
            Err(failure.error)
        }
    }
}

async fn run_inner() -> Result<(), StartupFailure> {
    let config = AppConfig::from_env().map_err(StartupFailure::before_logging)?;
    let log_outcome = match &config.log_file {
        LogFile::Resolved(path) => {
            crate::engine_logging::initialize(&LogSettings::with_defaults(path.to_path_buf()))
        }
        LogFile::Unresolved(reason) => {
            crate::engine_logging::initialize_terminal();
            LogInitOutcome {
                file_path: None,
                file_error: Some(reason.clone()),
            }
        }
    };
    if let Some(error) = &log_outcome.file_error {
        crate::engine_error!("file logging unavailable; using terminal logging only: {error}");
    }
    run_config(config, log_outcome)
        .await
        .map_err(StartupFailure::after_logging)
}

struct StartupFailure {
    error: StartupError,
    logging_initialized: bool,
}

impl StartupFailure {
    fn before_logging(error: impl Into<StartupError>) -> Self {
        Self {
            error: error.into(),
            logging_initialized: false,
        }
    }

    fn after_logging(error: StartupError) -> Self {
        Self {
            error,
            logging_initialized: true,
        }
    }
}

async fn run_config(config: AppConfig, log_outcome: LogInitOutcome) -> Result<(), StartupError> {
    crate::engine_info!("{}", startup_banner(&config, &log_outcome));
    let state = build_state(&config).await?;
    let router = router_for_assets(&config, state.clone())?;
    let address = config.socket_addr();
    let listener = tokio::net::TcpListener::bind(address)
        .await
        .map_err(|source| StartupError::PortUnavailable {
            address: address.to_string(),
            source,
        })?;
    crate::engine_info!(
        "backend listening on {}",
        listener
            .local_addr()
            .map_err(|source| StartupError::PortUnavailable {
                address: address.to_string(),
                source
            })?
    );
    let _refresh = spawn_launch_refresh(&config, state.clone());
    axum::serve(listener, router)
        .with_graceful_shutdown(shutdown_signal())
        .await
        .map_err(|source| StartupError::PortUnavailable {
            address: address.to_string(),
            source,
        })?;
    crate::engine_info!("backend shutdown complete");
    Ok(())
}

fn startup_banner(config: &AppConfig, log_outcome: &LogInitOutcome) -> String {
    let ledger = config
        .ledger
        .path
        .as_ref()
        .map(|path| path.display().to_string())
        .unwrap_or_else(|| "in-memory (demo)".to_owned());
    let log_path = log_outcome.file_path.as_ref().map_or_else(
        || {
            format!(
                "unavailable ({})",
                log_outcome.file_error.as_deref().unwrap_or("unknown error")
            )
        },
        |path| path.display().to_string(),
    );
    format!(
        "startup: mode={} ledger={} backup_dir={} static_assets_dir={} log={} listen={}",
        config.mode.as_str(),
        ledger,
        config.backup_dir.display(),
        config.static_assets_dir().display(),
        log_path,
        config.socket_addr(),
    )
}

fn router_for_assets(config: &AppConfig, state: AppState) -> Result<axum::Router, StartupError> {
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
        false if config.asset_policy() == AssetPolicy::Required => {
            Err(StartupError::StaticAssetsMissing {
                dir: config.static_assets_dir.clone(),
            })
        }
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
        crate::demo::seed(&pool)
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
                crate::market_data::RefreshTrigger::Launch,
                request,
            )
            .await
        {
            crate::engine_error!("launch refresh failed: {error}");
        }
    }))
}

async fn shutdown_signal() {
    match tokio::signal::ctrl_c().await {
        Ok(()) => crate::engine_info!("shutdown signal received"),
        // Signal registration failures are terminal for this local server path.
        Err(error) => crate::engine_error!("failed to listen for shutdown signal: {error}"),
    }
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

    use chrono::NaiveDate;
    use rust_decimal_macros::dec;
    use tokio::sync::Notify;

    use crate::{
        config::Mode,
        db::{self, instruments, provider_symbols, transactions},
        ledger::{memory, LedgerLocation},
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
            static_assets_dir: PathBuf::from("target/test-assets"),
            mode,
            create_ledger_if_missing: false,
            backup_enabled: false,
            backup_dir: crate::config::BackupDirectory::Unresolved("test".to_owned()),
            log_file: crate::config::LogFile::Unresolved("test".to_owned()),
            market_data_refresh_enabled: true,
            launch_refresh_enabled: true,
        }
    }

    #[test]
    fn startup_banner_names_in_memory_demo_ledger_on_one_line() {
        let config = test_config(Mode::Demo, memory());
        let log_outcome = LogInitOutcome {
            file_path: Some(PathBuf::from("C:/logs/engine-demo.log")),
            file_error: None,
        };

        let banner = startup_banner(&config, &log_outcome);

        assert!(banner.contains("mode=demo ledger=in-memory (demo)"));
        assert!(!banner.contains("ledger= backup_dir="));
        assert!(!banner.contains(['\r', '\n']));
    }

    #[test]
    fn startup_banner_reports_unavailable_file_logging_on_one_line() {
        let config = test_config(Mode::Production, memory());
        let log_outcome = LogInitOutcome {
            file_path: None,
            file_error: Some("invalid TTTB_LOG_FILE value".to_owned()),
        };

        let banner = startup_banner(&config, &log_outcome);

        assert!(banner.contains("log=unavailable (invalid TTTB_LOG_FILE value)"));
        assert!(!banner.contains(['\r', '\n']));
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
            .status(&state.pool)
            .await
            .expect("status should succeed");
        assert!(status.refreshing);
        assert_eq!(
            status.latest_run.expect("latest run").trigger,
            crate::market_data::RefreshTrigger::Launch
        );

        gate.notify_waiters();
        handle.await.expect("launch task should finish");

        let status = state
            .market_data
            .status(&state.pool)
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
        config.backup_dir = crate::config::BackupDirectory::Resolved(backup_directory.clone());

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

        let result = router_for_assets(&config, state);

        assert!(matches!(
            result,
            Err(StartupError::StaticAssetsMissing { .. })
        ));
        fs::remove_dir_all(directory).expect("test assets directory should be removed");
    }

    #[tokio::test]
    async fn optional_assets_build_an_api_only_router_for_an_empty_directory() {
        let directory = unique_assets_dir("optional");
        fs::create_dir_all(&directory).expect("test assets directory should be created");
        let mut config = test_config(Mode::Development, memory());
        config.static_assets_dir = directory.clone();
        let state = AppState::for_tests().await;

        let result = router_for_assets(&config, state);

        assert!(result.is_ok());
        fs::remove_dir_all(directory).expect("test assets directory should be removed");
    }

    fn unique_assets_dir(name: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time should be after UNIX_EPOCH")
            .as_nanos();
        PathBuf::from("target")
            .join("test-assets")
            .join(format!("{name}-{unique}"))
    }
}
