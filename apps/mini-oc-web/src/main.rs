//! mini-oc-web V2：BFF 服务入口（骨架）。

use axum::Router;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info".into()),
        )
        .init();

    let port: u16 = std::env::var("WEB_PORT")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(8100);

    let app = Router::new().fallback(|| async { "ok" });

    let addr = format!("0.0.0.0:{port}");
    let listener = tokio::net::TcpListener::bind(&addr).await.expect("bind failed");
    tracing::info!("mini-oc-web listening on {addr}");
    axum::serve(listener, app).await.expect("server error");
}
