fn main() {
    if let Err(error) = ticker_tape_tally_board_desktop::launch::run() {
        eprintln!("desktop startup failed: {error}");
        std::process::exit(1);
    }
}
