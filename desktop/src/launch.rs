use std::{error::Error, sync::Arc, time::Duration};

use ticker_tape_tally_board_backend::{
    api,
    config::{AppConfig, Mode},
    engine_logging, ledger,
    market_data::MarketDataService,
    state::AppState,
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
    let state = if probe_mode {
        handle.block_on(AppState::for_tests()).with_mode(Mode::Demo)
    } else {
        let config = AppConfig::from_env()?;
        let opened = handle.block_on(ledger::open(&config))?;
        AppState::new(opened.pool, Arc::new(MarketDataService::live()))
            .with_mode(config.mode)
            .with_ledger_path(config.ledger.path.clone())
            .with_backup(config.backup_dir.clone(), opened.launch_backup)
    };
    let assets = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../frontend/dist");
    let app_router = api::router_with_static_assets(assets, state);
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
    let result = tauri::Builder::default()
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
                handle.spawn(async move {
                    loop {
                        if let Some(code) = exit_probe.requested_exit_code() {
                            app_handle.exit(code);
                            break;
                        }
                        tokio::time::sleep(Duration::from_millis(50)).await;
                    }
                });
                let watchdog = watchdog_probe.clone();
                handle.spawn(async move {
                    tokio::time::sleep(Duration::from_secs(
                        webview_probe::watchdog_timeout_seconds(),
                    ))
                    .await;
                    webview_probe::watchdog_report(&watchdog);
                });
            }
            Ok(())
        })
        .build(tauri::generate_context!())?;
    result.run_return(|_, _| {});
    if probe_mode && probe.process_exit_code() != 0 {
        return Err("the WebView2 probe reported a failure".into());
    }
    Ok(())
}
