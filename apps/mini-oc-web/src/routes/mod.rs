use crate::state::AppState;
use axum::body::Body;
use axum::extract::State;
use axum::http::{header, StatusCode, Uri};
use axum::response::{IntoResponse, Response};
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
        .fallback(spa_fallback)
        .with_state(state)
}

async fn healthz() -> &'static str {
    "ok"
}

fn mime_for(path: &str) -> &'static str {
    match path.rsplit('.').next().unwrap_or("") {
        "html" => "text/html; charset=utf-8",
        "js" => "application/javascript",
        "css" => "text/css",
        "json" => "application/json",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "ico" => "image/x-icon",
        "woff2" => "font/woff2",
        "map" => "application/json",
        _ => "application/octet-stream",
    }
}

/// SPA 静态服务：物理文件优先，未命中路径回退 index.html（history 路由支持）。
/// /api 前缀不回退（显式 404，避免把接口错误当页面）。
async fn spa_fallback(State(state): State<SharedState>, uri: Uri) -> Response {
    let path = uri.path();
    if path.starts_with("/api") {
        return (
            StatusCode::NOT_FOUND,
            axum::Json(serde_json::json!({
                "error": {"code": "not_found", "message": format!("接口不存在: {path}")}
            })),
        )
            .into_response();
    }
    let dir = state.static_dir.clone();
    let rel = path.trim_start_matches('/');
    if !rel.is_empty() && !rel.contains("..") {
        let file = dir.join(rel);
        if file.is_file() {
            if let Ok(body) = tokio::fs::read(&file).await {
                return Response::builder()
                    .status(StatusCode::OK)
                    .header(header::CONTENT_TYPE, mime_for(rel))
                    .body(Body::from(body))
                    .unwrap();
            }
        }
    }
    // history 路由回退
    match tokio::fs::read(dir.join("index.html")).await {
        Ok(body) => Response::builder()
            .status(StatusCode::OK)
            .header(header::CONTENT_TYPE, "text/html; charset=utf-8")
            .body(Body::from(body))
            .unwrap(),
        Err(_) => (
            StatusCode::NOT_FOUND,
            "SPA 静态目录未构建（WEB_STATIC_DIR）",
        )
            .into_response(),
    }
}
