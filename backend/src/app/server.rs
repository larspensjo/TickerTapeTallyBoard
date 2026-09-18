use crate::{
    app::composition::Application,
    config::AppConfig,
    engine_logging::{initialize_for_shell, lifecycle_banner, LifecycleEvent, LogInitOutcome},
    startup_error::StartupError,
    state::AppShell,
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
    let log_outcome = initialize_for_shell(AppShell::Server, &config);
    crate::engine_info!(
        "{} listen={}",
        lifecycle_banner(
            LifecycleEvent::Startup,
            AppShell::Server,
            &config,
            &log_outcome
        ),
        config.socket_addr()
    );

    let mut application = Application::build(&config, config.asset_policy()).await?;
    let address = config.socket_addr();
    let listener = match tokio::net::TcpListener::bind(address).await {
        Ok(listener) => listener,
        Err(source) => {
            shutdown_application(&mut application, &config, &log_outcome).await;
            return Err(StartupError::PortUnavailable {
                address: address.to_string(),
                source,
            });
        }
    };
    let local_addr = match listener.local_addr() {
        Ok(local_addr) => local_addr,
        Err(source) => {
            shutdown_application(&mut application, &config, &log_outcome).await;
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
    shutdown_application(&mut application, &config, &log_outcome).await;
    result?;
    Ok(())
}

async fn shutdown_application(
    application: &mut Application,
    config: &AppConfig,
    log_outcome: &LogInitOutcome,
) {
    application.shutdown().await;
    crate::engine_info!(
        "{} listen={}",
        lifecycle_banner(
            LifecycleEvent::Shutdown,
            AppShell::Server,
            config,
            log_outcome
        ),
        config.socket_addr()
    );
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
