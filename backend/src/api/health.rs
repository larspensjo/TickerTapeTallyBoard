use axum::{extract::State, response::IntoResponse, Json};
use chrono::SecondsFormat;
use serde::Serialize;

use crate::{ledger::backup_status, state::AppState};

pub(super) async fn handler(State(state): State<AppState>) -> impl IntoResponse {
    Json(HealthResponse {
        status: "ok",
        version: env!("CARGO_PKG_VERSION"),
        mode: state.mode,
        ledger: LedgerInfo {
            mode: if state.ledger_path.is_some() {
                "file"
            } else {
                "memory"
            },
            path: state
                .ledger_path
                .as_ref()
                .map(|path| path.display().to_string()),
        },
        backup: BackupInfo::from_state(&state),
        build: BuildInfo {
            package: env!("CARGO_PKG_NAME"),
            profile: build_profile(),
        },
    })
}

fn build_profile() -> &'static str {
    if cfg!(debug_assertions) {
        "debug"
    } else {
        "release"
    }
}

#[derive(Debug, Serialize)]
struct HealthResponse {
    status: &'static str,
    version: &'static str,
    mode: crate::config::Mode,
    ledger: LedgerInfo,
    backup: BackupInfo,
    build: BuildInfo,
}

#[derive(Debug, Serialize)]
struct BuildInfo {
    package: &'static str,
    profile: &'static str,
}

#[derive(Debug, Serialize)]
struct LedgerInfo {
    mode: &'static str,
    path: Option<String>,
}

#[derive(Debug, Serialize)]
struct BackupInfo {
    directory: String,
    last_snapshot_at: Option<String>,
    snapshot_count: usize,
    launch_status: &'static str,
    launch_error: Option<String>,
    listing_error: Option<String>,
}

impl BackupInfo {
    fn from_state(state: &AppState) -> Self {
        let status = state
            .backup
            .directory
            .path()
            .map(backup_status)
            .unwrap_or_else(|| crate::ledger::BackupDirectoryStatus {
                last_snapshot_at: None,
                snapshot_count: 0,
                listing_error: Some(state.backup.directory.display()),
            });
        Self {
            directory: state.backup.directory.display(),
            last_snapshot_at: status
                .last_snapshot_at
                .map(|timestamp| timestamp.to_rfc3339_opts(SecondsFormat::Millis, true)),
            snapshot_count: status.snapshot_count,
            launch_status: state.backup.launch.status.as_str(),
            launch_error: state.backup.launch.error.clone(),
            listing_error: status.listing_error,
        }
    }
}

#[cfg(test)]
mod tests {
    use axum::{
        body::{to_bytes, Body},
        http::{Request, StatusCode},
    };
    use serde_json::Value;
    use std::{
        fs,
        path::PathBuf,
        time::{SystemTime, UNIX_EPOCH},
    };
    use tower::ServiceExt;

    #[tokio::test]
    async fn health_endpoint_returns_status_and_build_info() {
        let state = crate::state::AppState::for_tests()
            .await
            .with_ledger_path(Some(std::path::PathBuf::from("C:/target/portfolio.sqlite")));
        let response = crate::api::router(state)
            .oneshot(
                Request::builder()
                    .uri("/api/health")
                    .body(Body::empty())
                    .expect("request should build"),
            )
            .await
            .expect("request should complete");

        assert_eq!(response.status(), StatusCode::OK);

        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("body should be readable");
        let body: Value = serde_json::from_slice(&body).expect("body should be JSON");

        assert_eq!(body["status"], "ok");
        assert_eq!(body["version"], env!("CARGO_PKG_VERSION"));
        assert_eq!(body["mode"], "production");
        assert_eq!(body["ledger"]["mode"], "file");
        assert_eq!(body["ledger"]["path"], "C:/target/portfolio.sqlite");
        assert_eq!(body["backup"]["launch_status"], "skipped");
        assert!(body["backup"].get("launch_error").is_some());
        assert!(body["backup"].get("listing_error").is_some());
        assert!(body.get("demo").is_none());
        assert_eq!(body["build"]["package"], env!("CARGO_PKG_NAME"));
        assert!(body["build"]["profile"].is_string());
    }

    #[tokio::test]
    async fn health_serializes_backup_outcomes() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let directory = PathBuf::from("target/test-health-backups").join(unique.to_string());
        fs::create_dir_all(&directory).expect("backup directory");
        fs::write(directory.join("portfolio-20260829T143012000Z.sqlite"), [])
            .expect("snapshot marker");
        for (outcome, expected) in [
            (crate::ledger::LaunchBackupOutcome::succeeded(), "succeeded"),
            (
                crate::ledger::LaunchBackupOutcome::failed("disk full"),
                "failed",
            ),
            (crate::ledger::LaunchBackupOutcome::disabled(), "disabled"),
        ] {
            let state = crate::state::AppState::for_tests().await.with_backup(
                crate::config::BackupDirectory::Resolved(directory.clone()),
                outcome,
            );
            let response = crate::api::router(state)
                .oneshot(
                    Request::builder()
                        .uri("/api/health")
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
            let body: Value = serde_json::from_slice(&body).unwrap();
            assert_eq!(body["backup"]["launch_status"], expected);
            assert_eq!(body["backup"]["directory"], directory.display().to_string());
            assert_eq!(
                body["backup"]["last_snapshot_at"],
                "2026-08-29T14:30:12.000Z"
            );
            assert_eq!(body["backup"]["snapshot_count"], 1);
            assert_eq!(body["backup"]["listing_error"], Value::Null);
            if expected == "failed" {
                assert_eq!(body["backup"]["launch_error"], "disk full");
            } else {
                assert_eq!(body["backup"]["launch_error"], Value::Null);
            }
        }
        fs::remove_dir_all(directory).expect("remove backup directory");
    }

    #[tokio::test]
    async fn health_endpoint_returns_demo_ledger() {
        let state = crate::state::AppState::for_tests()
            .await
            .with_mode(crate::config::Mode::Demo);
        let response = crate::api::router(state)
            .oneshot(
                Request::builder()
                    .uri("/api/health")
                    .body(Body::empty())
                    .expect("request should build"),
            )
            .await
            .expect("request should complete");

        assert_eq!(response.status(), StatusCode::OK);

        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("body should be readable");
        let body: Value = serde_json::from_slice(&body).expect("body should be JSON");

        assert_eq!(body["mode"], "demo");
        assert_eq!(body["ledger"]["mode"], "memory");
        assert!(body["ledger"]["path"].is_null());
        assert!(body.get("demo").is_none());
    }
}
