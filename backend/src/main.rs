mod api;
mod app;
mod config;
mod db;
mod domain;
mod engine_logging;
mod import;
mod providers;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    engine_logging::initialize();

    app::serve(config::AppConfig::from_env()?).await
}
