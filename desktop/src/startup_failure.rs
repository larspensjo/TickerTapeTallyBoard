use std::{
    any::Any,
    error::Error,
    panic::PanicHookInfo,
    path::Path,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
};

use ticker_tape_tally_board_backend::{config::AppConfig, engine_logging::LogInitOutcome};

use crate::app_paths::AppPaths;

#[derive(Clone)]
pub struct DialogPaths {
    pub ledger: String,
    pub log: String,
}

impl DialogPaths {
    pub fn resolved(config: &AppConfig, outcome: &LogInitOutcome) -> Self {
        Self {
            ledger: config.ledger.path.as_ref().map_or_else(
                || "in-memory (demo)".to_owned(),
                |path| path.display().to_string(),
            ),
            log: outcome.file_path.as_ref().map_or_else(
                || {
                    format!(
                        "unavailable ({})",
                        outcome.file_error.as_deref().unwrap_or("unknown error")
                    )
                },
                |path| path.display().to_string(),
            ),
        }
    }

    pub fn configuration_failed() -> Self {
        Self {
            ledger: "unavailable (configuration failed)".to_owned(),
            log: "not initialized (configuration failed)".to_owned(),
        }
    }
}

pub struct LaunchFailure {
    pub message: String,
    paths: DialogPaths,
    logging_initialized: bool,
    show_dialog: bool,
    probe_outcome: bool,
}

impl LaunchFailure {
    pub fn before_configuration(error: impl Error, reason: &'static str) -> Self {
        let mut paths = DialogPaths::configuration_failed();
        paths.log = format!("not initialized ({reason})");
        Self {
            message: error.to_string(),
            paths,
            logging_initialized: false,
            show_dialog: true,
            probe_outcome: false,
        }
    }

    pub fn after_logging(error: impl Error, config: &AppConfig, outcome: &LogInitOutcome) -> Self {
        Self {
            message: error.to_string(),
            paths: DialogPaths::resolved(config, outcome),
            logging_initialized: true,
            show_dialog: true,
            probe_outcome: false,
        }
    }

    pub fn probe_outcome(
        message: impl Into<String>,
        config: &AppConfig,
        outcome: &LogInitOutcome,
    ) -> Self {
        Self {
            message: message.into(),
            paths: DialogPaths::resolved(config, outcome),
            logging_initialized: true,
            show_dialog: false,
            probe_outcome: true,
        }
    }

    pub fn report(&self, app_paths: &AppPaths) {
        if self.logging_initialized {
            if self.probe_outcome {
                ticker_tape_tally_board_backend::engine_error!(
                    "WebView2 probe failed: {}",
                    self.message
                );
            } else {
                ticker_tape_tally_board_backend::engine_error!("startup failed: {}", self.message);
            }
        }
        if self.show_dialog {
            show(
                &self.message,
                &app_paths.static_assets_dir,
                &self.paths.ledger,
                &self.paths.log,
            );
        }
    }
}

#[derive(Default)]
pub struct PanicReport {
    message: Mutex<Option<String>>,
    window_loop_started: AtomicBool,
}

impl PanicReport {
    pub fn install_hook(self: &Arc<Self>) {
        let report = Arc::clone(self);
        std::panic::set_hook(Box::new(move |info| {
            let message = panic_message(info);
            ticker_tape_tally_board_backend::engine_error!("desktop panic: {message}");
            if let Ok(mut saved) = report.message.lock() {
                *saved = Some(message);
            }
        }));
    }

    pub fn mark_window_loop_started(&self) {
        self.window_loop_started.store(true, Ordering::SeqCst);
    }

    pub fn message(&self) -> String {
        self.message
            .lock()
            .ok()
            .and_then(|message| message.clone())
            .unwrap_or_else(|| "panic details were unavailable".to_owned())
    }

    pub fn window_loop_started(&self) -> bool {
        self.window_loop_started.load(Ordering::SeqCst)
    }
}

pub fn show(error: &str, static_assets_dir: &Path, ledger: &str, log: &str) {
    show_message(&failure_message(
        false,
        error,
        static_assets_dir,
        ledger,
        log,
    ));
}

pub fn show_panic(report: &PanicReport, static_assets_dir: &Path, ledger: &str, log: &str) {
    show_message(&failure_message(
        report.window_loop_started(),
        &report.message(),
        static_assets_dir,
        ledger,
        log,
    ));
}

pub fn failure_message(
    window_loop_started: bool,
    error: &str,
    static_assets_dir: &Path,
    ledger: &str,
    log: &str,
) -> String {
    let heading = if window_loop_started {
        "TickerTapeTallyBoard stopped unexpectedly."
    } else {
        "TickerTapeTallyBoard could not start."
    };
    format!(
        "{heading}\n\n{error}\n\nLedger: {ledger}\nStatic assets: {}\nLog: {log}",
        static_assets_dir.display()
    )
}

fn panic_message(info: &PanicHookInfo<'_>) -> String {
    let payload = panic_payload(info.payload());
    match info.location() {
        Some(location) => format!(
            "{payload} at {}:{}:{}",
            location.file(),
            location.line(),
            location.column()
        ),
        None => payload,
    }
}

fn panic_payload(payload: &(dyn Any + Send)) -> String {
    if let Some(message) = payload.downcast_ref::<&str>() {
        (*message).to_owned()
    } else if let Some(message) = payload.downcast_ref::<String>() {
        message.clone()
    } else {
        "panic with a non-string payload".to_owned()
    }
}

fn show_message(message: &str) {
    let title: Vec<u16> = "TickerTapeTallyBoard failure\0".encode_utf16().collect();
    let body: Vec<u16> = format!("{message}\0").encode_utf16().collect();
    // SAFETY: MessageBoxW reads the null-terminated UTF-16 buffers only for this call.
    unsafe {
        windows_sys::Win32::UI::WindowsAndMessaging::MessageBoxW(
            std::ptr::null_mut(),
            body.as_ptr(),
            title.as_ptr(),
            windows_sys::Win32::UI::WindowsAndMessaging::MB_ICONERROR
                | windows_sys::Win32::UI::WindowsAndMessaging::MB_OK,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::failure_message;
    use std::path::Path;

    #[test]
    fn startup_failure_message_names_the_error_and_resolved_paths() {
        let message = failure_message(
            false,
            "ledger migration failed",
            Path::new("C:/source/frontend/dist"),
            "C:/data/portfolio.sqlite",
            "C:/data/logs/engine-desktop.log",
        );

        assert!(message.starts_with("TickerTapeTallyBoard could not start."));
        assert!(message.contains("ledger migration failed"));
        assert!(message.contains("Ledger: C:/data/portfolio.sqlite"));
        assert!(message.contains("Static assets: C:/source/frontend/dist"));
        assert!(message.contains("Log: C:/data/logs/engine-desktop.log"));
    }

    #[test]
    fn panic_message_distinguishes_a_running_window_loop() {
        let message = failure_message(
            true,
            "panic payload at launch.rs:1:2",
            Path::new("C:/assets"),
            "in-memory (demo)",
            "not initialized (configuration failed)",
        );

        assert!(message.starts_with("TickerTapeTallyBoard stopped unexpectedly."));
        assert!(message.contains("panic payload at launch.rs:1:2"));
    }
}
