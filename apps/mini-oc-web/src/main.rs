//! mini-oc-web V2：BFF 服务入口。

mod auth;
mod error;
mod routes;
mod sb;
mod state;

use std::sync::Arc;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();

    if let Err(e) = dotenvy::dotenv() {
        tracing::debug!("未加载 .env（正常，生产用环境变量）: {e}");
    }

    let port: u16 = std::env::var("WEB_PORT")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(8100);

    let state = match state::AppState::from_env() {
        Ok(s) => Arc::new(s),
        Err(e) => {
            tracing::error!("初始化失败: {e}");
            std::process::exit(1);
        }
    };

    // SB 登录 best-effort：失败仅告警，首次 401 会自动重登（V1 已验证的自愈模式）
    if let Err(e) = state.sb.login().await {
        tracing::warn!("启动时 SB 登录失败（稍后自动重试）: {e}");
    }

    let app = routes::build_router(state);
    let addr = format!("0.0.0.0:{port}");
    let listener = tokio::net::TcpListener::bind(&addr).await.expect("bind failed");
    tracing::info!("mini-oc-web v2 listening on {addr}");
    axum::serve(listener, app).await.expect("server error");
}
