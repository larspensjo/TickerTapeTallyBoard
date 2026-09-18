use std::{
    error::Error,
    panic::{self, AssertUnwindSafe},
    sync::Arc,
    time::Duration,
};

use tauri::RunEvent;
use tauri_plugin_window_state::StateFlags;
use ticker_tape_tally_board_backend::{
    app::Application,
    config::{AppConfig, AssetPolicy, BackupDirectory, LogFile, Mode},
    engine_logging::{initialize_for_shell, lifecycle_banner, LifecycleEvent},
    ledger,
    state::AppShell,
};

use crate::{
    app_paths::AppPaths,
    application_lifecycle::ApplicationLifecycle,
    in_process_http::{launch_url, serve_request, WEBVIEW_SCHEME},
    startup_failure::{self, LaunchFailure, PanicReport},
    webview_probe::{self, ProbeState},
};

pub fn run() -> Result<(), Box<dyn Error>> {
    let paths = AppPaths::from_environment();
    let runtime = match tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(error) => {
            let failure = LaunchFailure::before_configuration(error, "runtime creation failed");
            failure.report(&paths);
            return Err(failure.message.into());
        }
    };
    let lifecycle = Arc::new(ApplicationLifecycle::default());
    let panic_report = Arc::new(PanicReport::default());
    let result = panic::catch_unwind(AssertUnwindSafe(|| {
        run_inner(
            paths.clone(),
            runtime.handle(),
            Arc::clone(&lifecycle),
            Arc::clone(&panic_report),
        )
    }));

    match result {
        Ok(Ok(())) => Ok(()),
        Ok(Err(failure)) => {
            failure.report(&paths);
            Err(failure.message.into())
        }
        Err(_) => {
            lifecycle.shutdown(runtime.handle());
            let dialog_paths = lifecycle.dialog_paths();
            startup_failure::show_panic(
                &panic_report,
                &paths.static_assets_dir,
                &dialog_paths.ledger,
                &dialog_paths.log,
            );
            Err("desktop process panicked; details were recorded in the runtime log".into())
        }
    }
}

fn run_inner(
    paths: AppPaths,
    runtime: &tokio::runtime::Handle,
    lifecycle: Arc<ApplicationLifecycle>,
    panic_report: Arc<PanicReport>,
) -> Result<(), LaunchFailure> {
    let probe_mode = std::env::args()
        .skip(1)
        .any(|argument| argument == "--probe-webview");
    let config = if probe_mode {
        probe_config(paths.static_assets_dir.clone())
    } else {
        let mut config = AppConfig::from_env()
            .map_err(|error| LaunchFailure::before_configuration(error, "configuration failed"))?;
        config.static_assets_dir = paths.static_assets_dir.clone();
        config
    };

    let log_outcome = initialize_for_shell(AppShell::Desktop, &config);
    lifecycle.record_config(&config, &log_outcome);
    panic_report.install_hook();
    ticker_tape_tally_board_backend::engine_info!(
        "{}",
        lifecycle_banner(
            LifecycleEvent::Startup,
            AppShell::Desktop,
            &config,
            &log_outcome
        )
    );

    let mut built_application = runtime
        .block_on(Application::build(&config, AssetPolicy::Required))
        .map_err(|error| LaunchFailure::after_logging(error, &config, &log_outcome))?;
    built_application.start_launch_refresh(&config);
    let app_router = built_application.router.clone();
    lifecycle.record_application(built_application);

    let probe = ProbeState::new();
    let router = if probe_mode {
        webview_probe::wrap_with_state(app_router, probe.clone())
    } else {
        app_router
    };
    let protocol_router = router.clone();
    let protocol_runtime = runtime.clone();
    let protocol_probe = probe.clone();
    let version_probe = probe.clone();
    let exit_probe = probe.clone();
    let watchdog_probe = probe.clone();
    let setup_runtime = runtime.clone();
    let builder = tauri::Builder::default();
    let builder = if probe_mode {
        builder
    } else {
        builder.plugin(
            tauri_plugin_window_state::Builder::default()
                .with_state_flags(StateFlags::all() - StateFlags::VISIBLE)
                .build(),
        )
    };
    let app = builder
        .register_asynchronous_uri_scheme_protocol(
            WEBVIEW_SCHEME,
            move |_context, request, responder| {
                protocol_probe.record_uri(request.uri().to_string());
                let router = protocol_router.clone();
                protocol_runtime.spawn(async move {
                    responder.respond(serve_request(router, request).await);
                });
            },
        )
        .setup(move |app| {
            if probe_mode {
                version_probe.record_app_version(app.package_info().version.to_string());
            }
            let url = if probe_mode {
                webview_probe::harness_url()
            } else {
                launch_url()
            };
            let window = tauri::WebviewWindowBuilder::new(app, "main", url)
                .title("TickerTapeTallyBoard")
                .visible(probe_mode)
                .build()?;
            if !probe_mode {
                // Tauri queues plugin window-created callbacks during build().
                // This task is queued afterward, so FIFO restores placement first.
                app.run_on_main_thread(move || {
                    if let Err(error) = window.show() {
                        ticker_tape_tally_board_backend::engine_error!(
                            "failed to show restored desktop window: {error}"
                        );
                    }
                })?;
            }
            if probe_mode {
                let app_handle = app.handle().clone();
                let exit_probe = exit_probe.clone();
                setup_runtime.spawn(async move {
                    loop {
                        if let Some(code) = exit_probe.requested_exit_code() {
                            app_handle.exit(code);
                            break;
                        }
                        tokio::time::sleep(Duration::from_millis(50)).await;
                    }
                });
                let watchdog = watchdog_probe.clone();
                setup_runtime.spawn(async move {
                    tokio::time::sleep(Duration::from_secs(
                        webview_probe::watchdog_timeout_seconds(),
                    ))
                    .await;
                    webview_probe::watchdog_report(&watchdog);
                });
            }
            Ok(())
        })
        .build(tauri::generate_context!())
        .map_err(|error| {
            lifecycle.shutdown(runtime);
            LaunchFailure::after_logging(error, &config, &log_outcome)
        })?;

    let exit_lifecycle = Arc::clone(&lifecycle);
    let exit_runtime = runtime.clone();
    panic_report.mark_window_loop_started();
    app.run_return(move |_app_handle, event| {
        if matches!(event, RunEvent::Exit) {
            exit_lifecycle.shutdown(&exit_runtime);
        }
    });
    lifecycle.shutdown(runtime);
    if probe_mode && probe.process_exit_code() != 0 {
        return Err(LaunchFailure::probe_outcome(
            "the WebView2 probe reported a failure",
            &config,
            &log_outcome,
        ));
    }
    Ok(())
}

fn probe_config(static_assets_dir: std::path::PathBuf) -> AppConfig {
    AppConfig {
        host: "127.0.0.1".parse().expect("probe host is valid"),
        port: 8480,
        ledger: ledger::memory(),
        static_assets_dir,
        mode: Mode::Demo,
        create_ledger_if_missing: false,
        backup_enabled: false,
        backup_dir: BackupDirectory::Unresolved("probe mode".to_owned()),
        log_file: LogFile::Unresolved("probe mode".to_owned()),
        log_max_bytes: ticker_tape_tally_board_backend::engine_logging::DEFAULT_MAX_BYTES,
        market_data_refresh_enabled: false,
        launch_refresh_enabled: false,
    }
}
