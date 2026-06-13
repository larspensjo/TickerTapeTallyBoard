mod api;
mod db;
mod domain;
mod engine_logging;
mod import;
mod providers;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    engine_logging::initialize();

    let app = api::router();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:8080").await?;
    crate::engine_info!("backend listening on 127.0.0.1:8080");

    axum::serve(listener, app).await?;

    Ok(())
}
