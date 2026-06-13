use crate::config::AppConfig;

pub async fn serve(config: AppConfig) -> Result<(), Box<dyn std::error::Error>> {
    let address = config.socket_addr();
    let listener = tokio::net::TcpListener::bind(address).await?;
    let local_address = listener.local_addr()?;

    crate::engine_info!("backend listening on {local_address}");

    axum::serve(listener, crate::api::router())
        .with_graceful_shutdown(shutdown_signal())
        .await?;

    crate::engine_info!("backend shutdown complete");

    Ok(())
}

async fn shutdown_signal() {
    match tokio::signal::ctrl_c().await {
        Ok(()) => crate::engine_info!("shutdown signal received"),
        // Signal registration failures are terminal for this local server path.
        Err(error) => crate::engine_error!("failed to listen for shutdown signal: {error}"),
    }
}
