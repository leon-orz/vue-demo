//! hub HTTP API（S2-1 落地 /usage /config /control）

use axum::{routing::get, Router};

pub fn router() -> Router {
    Router::new().route("/api/v1/health", get(health))
}

async fn health() -> &'static str {
    "ok"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn health_returns_ok() {
        assert_eq!(health().await, "ok");
    }
}
