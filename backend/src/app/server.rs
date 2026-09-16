use crate::{
    app::composition::Application,
    config::{AppConfig, LogFile},
    engine_logging::{LogInitOutcome, LogSettings},
    startup_error::StartupError,
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
    serve(config).await.map_err(StartupFailure::after_logging)
}

pub async fn serve(config: AppConfig) -> Result<(), StartupError> {
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
    crate::engine_info!("{}", startup_banner(&config, &log_outcome));

    let mut application = Application::build(&config, config.asset_policy()).await?;
    let address = config.socket_addr();
    let listener = match tokio::net::TcpListener::bind(address).await {
        Ok(listener) => listener,
        Err(source) => {
            application.shutdown().await;
            return Err(StartupError::PortUnavailable {
                address: address.to_string(),
                source,
            });
        }
    };
    let local_addr = match listener.local_addr() {
        Ok(local_addr) => local_addr,
        Err(source) => {
            application.shutdown().await;
            return Err(StartupError::PortUnavailable {
                address: address.to_string(),
                source,
            });
        }
    };
    crate::engine_info!("backend listening on {}", local_addr);
    application.start_launch_refresh(&config);
    let result = axum::serve(listener, application.router.clone())
        .with_graceful_shutdown(shutdown_signal())
        .await
        .map_err(|source| StartupError::PortUnavailable {
            address: address.to_string(),
            source,
        });
    application.shutdown().await;
    result?;
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
        net::{IpAddr, Ipv4Addr},
        path::PathBuf,
    };

    use crate::{
        config::{LogFile, Mode},
        ledger::memory,
    };

    fn test_config(mode: Mode) -> AppConfig {
        AppConfig {
            host: IpAddr::V4(Ipv4Addr::LOCALHOST),
            port: 8480,
            ledger: memory(),
            static_assets_dir: PathBuf::from("test-assets"),
            mode,
            create_ledger_if_missing: false,
            backup_enabled: false,
            backup_dir: crate::config::BackupDirectory::Unresolved("test".to_owned()),
            log_file: LogFile::Unresolved("test".to_owned()),
            market_data_refresh_enabled: true,
            launch_refresh_enabled: true,
        }
    }

    #[test]
    fn startup_banner_names_in_memory_demo_ledger_on_one_line() {
        let config = test_config(Mode::Demo);
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
        let config = test_config(Mode::Production);
        let log_outcome = LogInitOutcome {
            file_path: None,
            file_error: Some("invalid TTTB_LOG_FILE value".to_owned()),
        };

        let banner = startup_banner(&config, &log_outcome);

        assert!(banner.contains("log=unavailable (invalid TTTB_LOG_FILE value)"));
        assert!(!banner.contains(['\r', '\n']));
    }
}
