use std::sync::Mutex;

use ticker_tape_tally_board_backend::{
    app::Application,
    config::AppConfig,
    engine_logging::{lifecycle_banner, LifecycleEvent, LogInitOutcome},
    state::AppShell,
};

use crate::startup_failure::DialogPaths;

#[derive(Clone)]
struct LifecycleContext {
    config: AppConfig,
    log_outcome: LogInitOutcome,
}

#[derive(Default)]
pub struct ApplicationLifecycle {
    application: Mutex<Option<Application>>,
    context: Mutex<Option<LifecycleContext>>,
}

impl ApplicationLifecycle {
    pub fn record_config(&self, config: &AppConfig, log_outcome: &LogInitOutcome) {
        *self.context.lock().expect("lifecycle context lock") = Some(LifecycleContext {
            config: config.clone(),
            log_outcome: log_outcome.clone(),
        });
    }

    pub fn record_application(&self, application: Application) {
        *self.application.lock().expect("application lock") = Some(application);
    }

    pub fn shutdown(&self, runtime: &tokio::runtime::Handle) {
        let application = self
            .application
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .take();
        if let Some(mut application) = application {
            runtime.block_on(application.shutdown());
            if let Some(context) = self
                .context
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .as_ref()
            {
                ticker_tape_tally_board_backend::engine_info!(
                    "{}",
                    lifecycle_banner(
                        LifecycleEvent::Shutdown,
                        AppShell::Desktop,
                        &context.config,
                        &context.log_outcome
                    )
                );
            }
        }
    }

    pub fn dialog_paths(&self) -> DialogPaths {
        self.context
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .as_ref()
            .map_or_else(DialogPaths::configuration_failed, |context| {
                DialogPaths::resolved(&context.config, &context.log_outcome)
            })
    }
}
