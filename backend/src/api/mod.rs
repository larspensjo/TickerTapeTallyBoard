use axum::{routing::get, Router};

pub fn router() -> Router {
    Router::new().route("/", get(root))
}

async fn root() -> &'static str {
    "TickerTapeTallyBoard backend"
}
