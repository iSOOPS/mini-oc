//! 路由注册中心。

use crate::state::AppState;
use axum::routing::get;
use axum::Router;
use std::sync::Arc;

pub mod admin_routes;
pub mod auth_routes;
pub mod device_routes;

pub type SharedState = Arc<AppState>;

pub fn build_router(state: SharedState) -> Router {
    Router::new()
        .route("/healthz", get(healthz))
        .merge(auth_routes::router())
        .merge(device_routes::router())
        .merge(admin_routes::router())
        .with_state(state)
}

async fn healthz() -> &'static str {
    "ok"
}
