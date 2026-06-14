use axum::{response::IntoResponse, Json};
use serde::Serialize;

pub(super) async fn handler() -> impl IntoResponse {
    // TODO: Remove after the Phase 0 importer spike replaces this placeholder.
    Json(SharesightSchemaPreview {
        status: "spike-placeholder",
        source: "Sharesight All Trades CSV",
        fields: vec![
            "market",
            "code",
            "name",
            "transaction_type",
            "trade_date",
            "quantity",
            "price",
            "instrument_currency",
            "cost_base_per_share_sek",
            "brokerage",
            "brokerage_currency",
            "exchange_rate",
            "value",
            "source_column",
            "comments",
        ],
        notes: vec![
            "Temporary endpoint for Phase 0 importer spike visibility.",
            "No private export rows are exposed by this endpoint.",
        ],
    })
}

#[derive(Debug, Serialize)]
struct SharesightSchemaPreview {
    status: &'static str,
    source: &'static str,
    fields: Vec<&'static str>,
    notes: Vec<&'static str>,
}

#[cfg(test)]
mod tests {
    use axum::{
        body::{to_bytes, Body},
        http::{Request, StatusCode},
    };
    use serde_json::Value;
    use tower::ServiceExt;

    #[tokio::test]
    async fn sharesight_schema_preview_exposes_expected_fields() {
        let state = crate::state::AppState::for_tests().await;
        let response = crate::api::router(state)
            .oneshot(
                Request::builder()
                    .uri("/api/import/sharesight/schema-preview")
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

        assert_eq!(body["source"], "Sharesight All Trades CSV");
        assert!(body["fields"]
            .as_array()
            .expect("fields should be an array")
            .contains(&Value::from("quantity")));
        assert!(body["fields"]
            .as_array()
            .expect("fields should be an array")
            .contains(&Value::from("exchange_rate")));
        assert!(body["notes"]
            .as_array()
            .expect("notes should be an array")
            .iter()
            .any(|note| {
                note.as_str()
                    .expect("note should be a string")
                    .contains("No private export rows")
            }));
    }
}
