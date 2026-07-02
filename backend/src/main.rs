use ticker_tape_tally_board_backend::{app, config, engine_logging};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    engine_logging::initialize();

    app::serve(config::AppConfig::from_env()?).await
}
