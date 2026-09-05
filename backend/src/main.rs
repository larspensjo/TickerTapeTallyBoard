use std::process::ExitCode;

#[tokio::main]
async fn main() -> ExitCode {
    match ticker_tape_tally_board_backend::app::run().await {
        Ok(()) => ExitCode::SUCCESS,
        Err(_) => ExitCode::FAILURE,
    }
}
