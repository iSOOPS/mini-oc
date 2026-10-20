//! Task 11：SPA 静态服务集成测试。

mod common;

use common::*;
use wiremock::{Mock, MockServer};

/// 构造带最小 SPA 目录的应用（模拟 dist）。
async fn spawn_with_dist(dist: &std::path::Path) -> String {
    std::fs::create_dir_all(dist).unwrap();
    std::fs::write(
        dist.join("index.html"),
        "<!doctype html><html><head><title>mini-oc</title></head><body><div id=app></div></body></html>",
    )
    .unwrap();
    std::fs::create_dir_all(dist.join("assets")).unwrap();
    std::fs::write(dist.join("assets/app.js"), "console.log(1)").unwrap();
    let sb = MockServer::start().await;
    mount_sb_users(&sb, &users_json(vec![])).await;
    spawn_app_with_static(&sb, Some(dist.to_str().unwrap().to_string())).await
}

#[tokio::test]
async fn spa_root_returns_index_html() {
    let dist = std::env::temp_dir().join(format!("mini-oc-spa-{}", std::process::id()));
    let base = spawn_with_dist(&dist).await;
    let resp = reqwest::get(format!("{base}/")).await.unwrap();
    assert_eq!(resp.status(), 200);
    assert!(resp.text().await.unwrap().contains("mini-oc"));
    let _ = std::fs::remove_dir_all(&dist);
}

#[tokio::test]
async fn spa_history_fallback_returns_index_for_unknown_paths() {
    let dist = std::env::temp_dir().join(format!("mini-oc-spa-h-{}", std::process::id()));
    let base = spawn_with_dist(&dist).await;
    // /devices 是前端路由，无物理文件 → 回退 index.html
    let resp = reqwest::get(format!("{base}/devices")).await.unwrap();
    assert_eq!(resp.status(), 200);
    assert!(resp.text().await.unwrap().contains("mini-oc"));
    let _ = std::fs::remove_dir_all(&dist);
}

#[tokio::test]
async fn spa_serves_real_assets() {
    let dist = std::env::temp_dir().join(format!("mini-oc-spa-a-{}", std::process::id()));
    let base = spawn_with_dist(&dist).await;
    let resp = reqwest::get(format!("{base}/assets/app.js")).await.unwrap();
    assert_eq!(resp.status(), 200);
    let _ = std::fs::remove_dir_all(&dist);
}

#[tokio::test]
async fn api_routes_still_work_alongside_spa() {
    let dist = std::env::temp_dir().join(format!("mini-oc-spa-api-{}", std::process::id()));
    let base = spawn_with_dist(&dist).await;
    let resp = reqwest::get(format!("{base}/healthz")).await.unwrap();
    assert_eq!(resp.status(), 200);
    assert_eq!(resp.text().await.unwrap(), "ok");
    let _ = std::fs::remove_dir_all(&dist);
}
