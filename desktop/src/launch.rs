use std::{error::Error, path::PathBuf, time::Duration};

use ticker_tape_tally_board_backend::{
    app::Application,
    config::{AppConfig, AssetPolicy, BackupDirectory, LogFile, Mode},
    engine_logging, ledger,
};

use crate::{
    in_process_http::{launch_url, serve_request, WEBVIEW_SCHEME},
    webview_probe::{self, ProbeState},
};

pub fn run() -> Result<(), Box<dyn Error>> {
    let probe_mode = std::env::args()
        .skip(1)
        .any(|argument| argument == "--probe-webview");
    engine_logging::initialize_terminal();
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;
    let handle = runtime.handle().clone();
    let config = if probe_mode {
        probe_config()
    } else {
        let mut config = AppConfig::from_env()?;
        config.static_assets_dir = desktop_assets_dir();
        config
    };
    let mut application = handle.block_on(Application::build(&config, AssetPolicy::Required))?;
    application.start_launch_refresh(&config);
    let app_router = application.router.clone();
    let probe = ProbeState::new();
    let router = if probe_mode {
        webview_probe::wrap_with_state(app_router, probe.clone())
    } else {
        app_router
    };

    let protocol_router = router.clone();
    let protocol_runtime = handle.clone();
    let protocol_probe = probe.clone();
    let version_probe = probe.clone();
    let exit_probe = probe.clone();
    let watchdog_probe = probe.clone();
    let setup_runtime = handle.clone();
    let result = match tauri::Builder::default()
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
            tauri::WebviewWindowBuilder::new(app, "main", url)
                .title("TickerTapeTallyBoard")
                .build()?;
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
    {
        Ok(result) => result,
        Err(error) => {
            handle.block_on(application.shutdown());
            return Err(error.into());
        }
    };
    result.run_return(|_, _| {});
    handle.block_on(application.shutdown());
    if probe_mode && probe.process_exit_code() != 0 {
        return Err("the WebView2 probe reported a failure".into());
    }
    Ok(())
}

fn desktop_assets_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../frontend/dist")
}

fn probe_config() -> AppConfig {
    AppConfig {
        host: "127.0.0.1".parse().expect("probe host is valid"),
        port: 8480,
        ledger: ledger::memory(),
        static_assets_dir: desktop_assets_dir(),
        mode: Mode::Demo,
        create_ledger_if_missing: false,
        backup_enabled: false,
        backup_dir: BackupDirectory::Unresolved("probe mode".to_owned()),
        log_file: LogFile::Unresolved("probe mode".to_owned()),
        market_data_refresh_enabled: false,
        launch_refresh_enabled: false,
    }
}
