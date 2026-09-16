use std::{
    collections::BTreeSet,
    fs,
    path::PathBuf,
    sync::{Arc, Mutex},
    time::{SystemTime, UNIX_EPOCH},
};

use axum::{
    extract::DefaultBodyLimit,
    http::StatusCode,
    response::IntoResponse,
    routing::{delete, get, post},
    Json, Router,
};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use ticker_tape_tally_board_backend::api::{
    body_limits::{self, IMPORT_BODY_LIMIT_BYTES},
    extract::ImportBody,
};

use crate::in_process_http::browser_visible_origin;

pub const CASE_TIMEOUT_SECONDS: u64 = 60;
pub const PAGE_AND_REPORT_SLACK_SECONDS: u64 = 20;
pub const CSP_VIOLATION_TIMEOUT_MS: u64 = 1_000;
pub const CASE_NAMES: [&str; 8] = [
    "rejected_write",
    "no_content",
    "large_body",
    "oversized_body",
    "health",
    "connect_src",
    "script_src",
    "uri_form",
];

pub const fn watchdog_timeout_seconds() -> u64 {
    CASE_TIMEOUT_SECONDS * CASE_NAMES.len() as u64 + PAGE_AND_REPORT_SLACK_SECONDS
}

#[derive(Clone)]
pub struct ProbeState {
    observed_uri: Arc<Mutex<Option<String>>>,
    app_version: Arc<Mutex<Option<String>>>,
    outcome: Arc<Mutex<Option<ProbeOutcome>>>,
}

#[derive(Clone, Copy)]
struct ProbeOutcome {
    complete: bool,
    all_cases_passed: bool,
    report_written: bool,
}

impl ProbeState {
    pub fn new() -> Self {
        Self {
            observed_uri: Arc::new(Mutex::new(None)),
            app_version: Arc::new(Mutex::new(None)),
            outcome: Arc::new(Mutex::new(None)),
        }
    }

    pub fn record_uri(&self, uri: impl Into<String>) {
        let mut observed = self.observed_uri.lock().expect("probe URI lock");
        if observed.is_none() {
            *observed = Some(uri.into());
        }
    }

    pub fn record_app_version(&self, version: impl Into<String>) {
        *self.app_version.lock().expect("probe version lock") = Some(version.into());
    }

    pub fn requested_exit_code(&self) -> Option<i32> {
        self.recorded_outcome()
            .map(|outcome| probe_exit_code(Some(outcome)))
    }

    pub fn process_exit_code(&self) -> i32 {
        probe_exit_code(self.recorded_outcome())
    }

    fn recorded_outcome(&self) -> Option<ProbeOutcome> {
        *self.outcome.lock().expect("probe outcome lock")
    }

    fn record_outcome(&self, outcome: ProbeOutcome) {
        *self.outcome.lock().expect("probe outcome lock") = Some(outcome);
    }
}

impl Default for ProbeState {
    fn default() -> Self {
        Self::new()
    }
}

/// Adds diagnostic-only routes around, never into, the application router.
pub fn wrap(router: Router) -> Router {
    wrap_with_state(router, ProbeState::new())
}

pub fn wrap_with_state(router: Router, state: ProbeState) -> Router {
    Router::new()
        .route("/__probe", get(harness_page))
        .route("/__probe/probe.js", get(harness_script))
        .route(
            "/__probe/echo",
            post(echo).layer(DefaultBodyLimit::max(IMPORT_BODY_LIMIT_BYTES)),
        )
        .route(
            "/__probe/echo-limited",
            post(echo).layer(DefaultBodyLimit::max(IMPORT_BODY_LIMIT_BYTES)),
        )
        .route("/__probe/no-content", delete(no_content))
        .route(
            "/__probe/report",
            post(move |payload| report(state.clone(), payload)),
        )
        .fallback_service(router)
}

pub fn harness_url() -> tauri::WebviewUrl {
    let mut url = match crate::in_process_http::launch_url() {
        tauri::WebviewUrl::CustomProtocol(url) => url,
        _ => unreachable!("launch URL is custom"),
    };
    url.set_path("/__probe");
    tauri::WebviewUrl::CustomProtocol(url)
}

async fn harness_page() -> impl IntoResponse {
    (
        [("content-type", "text/html; charset=utf-8")],
        "<!doctype html><html><head><title>WebView2 transport probe</title></head><body><h1>WebView2 transport probe</h1><table><thead><tr><th>Case</th><th>Result</th><th>Elapsed</th><th>Reason</th></tr></thead><tbody id=results></tbody></table><script src=\"/__probe/probe.js\"></script></body></html>",
    )
}

async fn harness_script() -> impl IntoResponse {
    let body = format!(
        r#"const CASE_TIMEOUT_MS = {timeout};
const UNDER_LIMIT_BYTES = {under};
const OVER_LIMIT_BYTES = {over};
const CSP_VIOLATION_TIMEOUT_MS = {violation_timeout};
const results = [];
function render(result) {{
  const row = document.createElement('tr');
  for (const value of [result.name, result.passed ? 'PASS' : 'FAIL', `${{result.elapsed_ms}} ms`, result.reason || '']) {{
    const cell = document.createElement('td'); cell.textContent = value; row.appendChild(cell);
  }}
  document.getElementById('results').appendChild(row);
}}
async function run(name, operation) {{
  const started = performance.now();
  let timer;
  try {{
    const outcome = await Promise.race([
      operation(),
      new Promise((_, reject) => {{ timer = setTimeout(() => reject(new Error('timeout')), CASE_TIMEOUT_MS); }})
    ]);
    const result = {{ name, passed: true, elapsed_ms: Math.round(performance.now() - started), ...outcome }};
    results.push(result); render(result);
  }} catch (error) {{
    const result = {{ name, passed: false, elapsed_ms: Math.round(performance.now() - started), reason: error.message || String(error) }};
    results.push(result); render(result);
  }} finally {{ clearTimeout(timer); }}
}}
function bytes(length) {{ const value = new Uint8Array(length); for (let i = 0; i < value.length; i++) value[i] = i % 251; return value; }}
async function sha256(value) {{ const digest = await crypto.subtle.digest('SHA-256', value); return Array.from(new Uint8Array(digest)).map(b => b.toString(16).padStart(2, '0')).join(''); }}
function violation(expectedDirectives, expectedBlocked, missingMessage) {{
  return new Promise((resolve, reject) => {{
    const timer = setTimeout(() => {{ document.removeEventListener('securitypolicyviolation', listener); reject(new Error(missingMessage)); }}, CSP_VIOLATION_TIMEOUT_MS);
    const listener = event => {{ if (expectedDirectives.includes(event.effectiveDirective) && (expectedBlocked === 'inline' ? event.blockedURI === 'inline' : event.blockedURI.startsWith(expectedBlocked))) {{ clearTimeout(timer); document.removeEventListener('securitypolicyviolation', listener); resolve(); }} }};
    document.addEventListener('securitypolicyviolation', listener);
  }});
}}
(async () => {{
  await run('rejected_write', async () => {{ const response = await fetch('/api/transactions', {{method:'POST', headers:{{'content-type':'application/json'}}, body:'{{}}'}}); const body = await response.json(); if (response.status !== 403 || body.error?.code !== 'demo_read_only') throw new Error(`status=${{response.status}} code=${{body.error?.code}}`); return {{status:response.status}}; }});
  await run('no_content', async () => {{ const response = await fetch('/__probe/no-content', {{method:'DELETE'}}); const text = await response.text(); const length = response.headers.get('content-length'); if (response.status !== 204 || text !== '' || (length !== null && length !== '0')) throw new Error(`status=${{response.status}} body=${{text.length}} content-length=${{length}}`); return {{status:response.status}}; }});
  await run('large_body', async () => {{ const body = bytes(UNDER_LIMIT_BYTES); const expected = await sha256(body); const response = await fetch('/__probe/echo', {{method:'POST', headers:{{'content-type':'text/csv'}}, body}}); const payload = await response.json(); if (response.status !== 200 || payload.received_bytes !== body.byteLength || payload.sha256 !== expected) throw new Error(`status=${{response.status}} bytes=${{payload.received_bytes}} digest=${{payload.sha256}}`); return {{status:response.status}}; }});
  await run('oversized_body', async () => {{ const response = await fetch('/__probe/echo-limited', {{method:'POST', headers:{{'content-type':'text/csv'}}, body:bytes(OVER_LIMIT_BYTES)}}); const payload = await response.json(); if (response.status !== 413 || payload.error?.code !== 'payload_too_large') throw new Error(`status=${{response.status}} code=${{payload.error?.code}}`); return {{status:response.status}}; }});
  await run('health', async () => {{ const response = await fetch('/api/health'); const body = await response.json(); if (response.status !== 200 || !body.status) throw new Error(`status=${{response.status}}`); return {{status:response.status}}; }});
  await run('connect_src', async () => {{ const event = violation(['connect-src'], 'https://example.invalid', 'missing connect-src violation'); let rejected = false; try {{ await fetch('https://example.invalid/'); }} catch (_) {{ rejected = true; }} await event; if (!rejected) throw new Error('fetch unexpectedly resolved'); return {{}}; }});
  await run('script_src', async () => {{ const event = violation(['script-src', 'script-src-elem'], 'inline', 'missing script-src violation'); window.__probeInlineRan = false; const script = document.createElement('script'); script.text = 'window.__probeInlineRan = true'; document.body.appendChild(script); await event; if (window.__probeInlineRan) throw new Error('inline script executed'); return {{}}; }});
  await run('uri_form', async () => {{ return {{ origin: location.origin }}; }});
  await fetch('/__probe/report', {{method:'POST', headers:{{'content-type':'application/json'}}, body:JSON.stringify({{ complete:true, cases:results, location_origin:location.origin, body_sizes:{{under_limit_bytes:UNDER_LIMIT_BYTES, over_limit_bytes:OVER_LIMIT_BYTES}} }})}});
}})();"#,
        timeout = CASE_TIMEOUT_SECONDS * 1000,
        under = body_limits::under_import_limit_bytes(),
        over = body_limits::over_import_limit_bytes(),
        violation_timeout = CSP_VIOLATION_TIMEOUT_MS,
    );
    (
        [("content-type", "application/javascript; charset=utf-8")],
        body,
    )
}

async fn echo(ImportBody(body): ImportBody) -> Json<Value> {
    let mut digest = Sha256::new();
    digest.update(&body);
    let digest = digest.finalize();
    let sha256 = digest
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    Json(json!({ "received_bytes": body.len(), "sha256": sha256 }))
}

async fn no_content() -> StatusCode {
    StatusCode::NO_CONTENT
}

async fn report(state: ProbeState, Json(mut payload): Json<Value>) -> impl IntoResponse {
    let observed = state.observed_uri.lock().expect("probe URI lock").clone();
    let app_version = state
        .app_version
        .lock()
        .expect("probe version lock")
        .clone()
        .unwrap_or_else(|| env!("CARGO_PKG_VERSION").to_owned());
    payload["handler_observed_uri"] = json!(observed);
    payload["browser_visible_origin"] = json!(browser_visible_origin());
    payload["import_body_limit_bytes"] = json!(IMPORT_BODY_LIMIT_BYTES);
    payload["versions"] = versions(app_version);
    let complete = payload["complete"].as_bool().unwrap_or(false);
    let passed = payload["cases"].as_array().is_some_and(|cases| {
        cases
            .iter()
            .all(|case| case["passed"].as_bool() == Some(true))
    });
    let report_written = write_report(&payload);
    if let Some(cases) = payload["cases"].as_array() {
        for case in cases {
            ticker_tape_tally_board_backend::engine_info!(
                "webview probe case {}: {}",
                case["name"].as_str().unwrap_or("unknown"),
                if case["passed"].as_bool() == Some(true) {
                    "passed"
                } else {
                    "failed"
                }
            );
        }
    }
    state.record_outcome(ProbeOutcome {
        complete,
        all_cases_passed: passed,
        report_written,
    });
    StatusCode::NO_CONTENT
}

pub fn watchdog_report(state: &ProbeState) {
    if state.recorded_outcome().is_some() {
        return;
    }
    let missing: BTreeSet<_> = CASE_NAMES.into_iter().collect();
    let payload = json!({
        "complete": false,
        "reason": "watchdog_timeout",
        "cases_never_reported": missing,
        "watchdog_timeout_seconds": watchdog_timeout_seconds(),
        "import_body_limit_bytes": IMPORT_BODY_LIMIT_BYTES,
        "body_sizes": { "under_limit_bytes": body_limits::under_import_limit_bytes(), "over_limit_bytes": body_limits::over_import_limit_bytes() },
        "versions": versions(env!("CARGO_PKG_VERSION")),
    });
    ticker_tape_tally_board_backend::engine_error!(
        "webview probe watchdog fired; no report arrived within {} seconds",
        watchdog_timeout_seconds()
    );
    let report_written = write_report(&payload);
    state.record_outcome(ProbeOutcome {
        complete: false,
        all_cases_passed: false,
        report_written,
    });
}

fn probe_exit_code(outcome: Option<ProbeOutcome>) -> i32 {
    match outcome {
        Some(ProbeOutcome {
            complete: true,
            all_cases_passed: true,
            report_written: true,
        }) => 0,
        _ => 1,
    }
}

fn versions(app_version: impl Into<String>) -> Value {
    json!({
        "tauri": env!("PROBE_TAURI_VERSION"),
        "wry": env!("PROBE_WRY_VERSION"),
        "webview2_com": env!("PROBE_WEBVIEW2_COM_VERSION"),
        "webview2_runtime": tauri::webview_version().unwrap_or_else(|error| format!("unavailable: {error}")),
        "app": app_version.into(),
    })
}

fn write_report(payload: &Value) -> bool {
    let directory = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join(".local/probe");
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_millis();
    if let Err(error) = fs::create_dir_all(&directory).and_then(|()| {
        fs::write(
            directory.join(format!("webview-probe-{timestamp}.json")),
            serde_json::to_vec_pretty(payload).expect("probe JSON"),
        )
    }) {
        ticker_tape_tally_board_backend::engine_error!(
            "webview probe could not write its report: {error}"
        );
        false
    } else {
        true
    }
}

#[cfg(test)]
mod tests {
    use super::{probe_exit_code, ProbeOutcome};

    #[test]
    fn complete_report_with_all_cases_passing_exits_zero() {
        assert_eq!(
            probe_exit_code(Some(ProbeOutcome {
                complete: true,
                all_cases_passed: true,
                report_written: true,
            })),
            0
        );
    }

    #[test]
    fn failed_case_exits_non_zero() {
        assert_ne!(
            probe_exit_code(Some(ProbeOutcome {
                complete: true,
                all_cases_passed: false,
                report_written: true,
            })),
            0
        );
    }

    #[test]
    fn incomplete_report_exits_non_zero() {
        assert_ne!(
            probe_exit_code(Some(ProbeOutcome {
                complete: false,
                all_cases_passed: true,
                report_written: true,
            })),
            0
        );
    }

    #[test]
    fn missing_report_exits_non_zero() {
        assert_ne!(probe_exit_code(None), 0);
    }
}
