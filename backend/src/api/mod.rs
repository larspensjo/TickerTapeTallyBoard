mod cors;
mod health;
mod root;
mod sharesight;

use axum::{routing::get, Router};

pub fn router() -> Router {
    Router::new()
        .route("/", get(root::handler))
        .nest(
            "/api",
            Router::new().route("/health", get(health::handler)).route(
                "/import/sharesight/schema-preview",
                get(sharesight::handler),
            ),
        )
        .layer(cors::layer())
}
