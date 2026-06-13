#[macro_export]
macro_rules! engine_trace {
    ($($arg:tt)*) => {{
        log::trace!($($arg)*);
    }};
}

#[macro_export]
macro_rules! engine_info {
    ($($arg:tt)*) => {{
        log::info!($($arg)*);
    }};
}

#[macro_export]
macro_rules! engine_debug {
    ($($arg:tt)*) => {{
        log::debug!($($arg)*);
    }};
}

#[macro_export]
macro_rules! engine_warn {
    ($($arg:tt)*) => {{
        log::warn!($($arg)*);
    }};
}

#[macro_export]
macro_rules! engine_error {
    ($($arg:tt)*) => {{
        log::error!($($arg)*);
    }};
}

pub fn initialize() {
    use simplelog::{ColorChoice, CombinedLogger, Config, TermLogger, TerminalMode, WriteLogger};
    use std::fs::OpenOptions;

    let level = log::LevelFilter::Info;
    let log_file = OpenOptions::new()
        .create(true)
        .append(true)
        .open("engine.log")
        .expect("Failed to open engine.log");

    let _ = CombinedLogger::init(vec![
        TermLogger::new(
            level,
            Config::default(),
            TerminalMode::Stderr,
            ColorChoice::Auto,
        ),
        WriteLogger::new(level, Config::default(), log_file),
    ]);
}
