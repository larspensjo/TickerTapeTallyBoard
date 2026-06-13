use axum::http::Method;
use tower_http::cors::{Any, CorsLayer};

pub(super) fn layer() -> CorsLayer {
    // Broad local-dev CORS keeps direct frontend calls working while ports vary.
    // Add POST here when the import endpoints arrive.
    CorsLayer::new()
        .allow_origin(Any)
        .allow_methods([Method::GET])
        .allow_headers(Any)
}
