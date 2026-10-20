//! Task 8：项目/会话双源 + SB 快照测试。

mod common;

use common::*;
use wiremock::matchers::{method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

/// 登录后返回 cookie 前缀（name=value）。
async fn login_cookie(base: &str, key: &str) -> String {
    reqwest::Client::new()
        .post(format!("{base}/api/login"))
        .json(&serde_json::json!({"key": key}))
        .send()
        .await
        .unwrap()
        .headers()
        .get("set-cookie")
        .unwrap()
        .to_str()
        .unwrap()
        .split(';')
        .next()
        .unwrap()
        .to_string()
}

fn project_response() -> serde_json::Value {
    serde_json::json!([
        {"id": "p1", "canonical": "/Users/x/proj-a", "vcs": "git",
         "time": {"created": 1, "updated": 2, "active": 3}}
    ])
}

fn session_response() -> serde_json::Value {
    serde_json::json!({
        "data": [
            {"id": "ses_1", "projectID": "p1", "cost": 0,
             "tokens": {"input": 1, "output": 2, "reasoning": 0, "cache": {"read": 0, "write": 0}},
             "time": {"created": 1, "updated": 2},
             "title": "会话一", "location": {"directory": "/Users/x/proj-a"}}
        ],
        "cursor": {"previous": null, "next": null}
    })
}

#[tokio::test]
async fn projects_live_success_writes_snapshot() {
    let sb = MockServer::start().await;
    let device = MockServer::start().await;
    Mock::given(method("GET")).and(path("/api/project"))
        .respond_with(ResponseTemplate::new(200).set_body_json(project_response()))
        .mount(&device).await;
    let mut user = test_user();
    user.devices[0].public_url = device.uri();
    mount_sb_users(&sb, &users_json(vec![user.clone()])).await;
    // 快照写回断言
    Mock::given(method("PUT"))
        .and(path("/.fs/serv/opencode/snapshots/100001/macbook-pro/projects.md"))
        .and(wiremock::matchers::body_string_contains("\"id\":\"p1\""))
        .respond_with(ResponseTemplate::new(200))
        .expect(1)
        .mount(&sb).await;
    let base = spawn_app(&sb).await;
    let cookie = login_cookie(&base, &user.key).await;

    let resp = reqwest::Client::new()
        .get(format!("{base}/api/devices/macbook-pro/projects"))
        .header("cookie", &cookie)
        .send().await.unwrap();
    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["source"], "live");
    assert_eq!(body["data"][0]["id"], "p1");
    assert!(body["fetchedAt"].is_null(), "live 无需快照时间");
    // 快照被异步写入——verify 等待
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    sb.verify().await;
}

#[tokio::test]
async fn projects_device_401_falls_back_to_snapshot() {
    let sb = MockServer::start().await;
    // 预置快照（设备曾经在线时写入）
    let snapshot = serde_json::json!({
        "fetched_at": "2026-10-20T10:00:00Z",
        "data": [
            {"id": "p_old", "canonical": "/Users/x/old", "vcs": null,
             "time": {"created": 1, "updated": 2, "active": 3}}
        ]
    });
    Mock::given(method("GET"))
        .and(path("/.fs/serv/opencode/snapshots/100001/macbook-pro/projects.md"))
        .respond_with(ResponseTemplate::new(200).set_body_json(snapshot))
        .mount(&sb).await;
    let mut user = test_user();
    // 设备 401（auth_mismatch）
    let device = MockServer::start().await;
    Mock::given(method("GET")).and(path("/api/project"))
        .respond_with(ResponseTemplate::new(401))
        .mount(&device).await;
    user.devices[0].public_url = device.uri();
    mount_sb_users(&sb, &users_json(vec![user.clone()])).await;
    let base = spawn_app(&sb).await;
    let cookie = login_cookie(&base, &user.key).await;

    let resp = reqwest::Client::new()
        .get(format!("{base}/api/devices/macbook-pro/projects"))
        .header("cookie", &cookie)
        .send().await.unwrap();
    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["source"], "snapshot", "auth_mismatch 走快照回退");
    assert_eq!(body["data"][0]["id"], "p_old");
    assert_eq!(body["fetchedAt"], "2026-10-20T10:00:00Z");
}

#[tokio::test]
async fn sessions_device_offline_no_snapshot_returns_empty_with_marker() {
    let sb = MockServer::start().await;
    let mut user = test_user();
    user.devices[0].public_url = "http://127.0.0.1:1".into(); // 不通
    // 无快照（GET 404）
    Mock::given(method("GET"))
        .and(path("/.fs/serv/opencode/snapshots/100001/macbook-pro/sessions.md"))
        .respond_with(ResponseTemplate::new(404))
        .mount(&sb).await;
    mount_sb_users(&sb, &users_json(vec![user.clone()])).await;
    let base = spawn_app(&sb).await;
    let cookie = login_cookie(&base, &user.key).await;

    let resp = reqwest::Client::new()
        .get(format!("{base}/api/devices/macbook-pro/sessions"))
        .header("cookie", &cookie)
        .send().await.unwrap();
    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["source"], "snapshot");
    assert_eq!(body["data"].as_array().unwrap().len(), 0, "无快照时空列表");
    assert!(body["fetchedAt"].is_null(), "无快照时 fetchedAt=null");
}

#[tokio::test]
async fn live_result_cached_60s_no_second_device_hit() {
    let sb = MockServer::start().await;
    let device = MockServer::start().await;
    Mock::given(method("GET")).and(path("/api/project"))
        .respond_with(ResponseTemplate::new(200).set_body_json(project_response()))
        // 60s 缓存窗口内只打一次设备
        .expect(1)
        .mount(&device).await;
    let mut user = test_user();
    user.devices[0].public_url = device.uri();
    mount_sb_users(&sb, &users_json(vec![user.clone()])).await;
    let base = spawn_app(&sb).await;
    let cookie = login_cookie(&base, &user.key).await;

    for _ in 0..3 {
        let resp = reqwest::Client::new()
            .get(format!("{base}/api/devices/macbook-pro/projects"))
            .header("cookie", &cookie)
            .send().await.unwrap();
        assert_eq!(resp.status(), 200);
    }
    device.verify().await;
}

#[tokio::test]
async fn sessions_live_passes_directory_param() {
    let sb = MockServer::start().await;
    let device = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/session"))
        .and(query_param("directory", "/Users/x/proj-a"))
        .respond_with(ResponseTemplate::new(200).set_body_json(session_response()))
        .expect(1)
        .mount(&device).await;
    let mut user = test_user();
    user.devices[0].public_url = device.uri();
    mount_sb_users(&sb, &users_json(vec![user.clone()])).await;
    let base = spawn_app(&sb).await;
    let cookie = login_cookie(&base, &user.key).await;

    let resp = reqwest::Client::new()
        .get(format!("{base}/api/devices/macbook-pro/sessions?directory=/Users/x/proj-a"))
        .header("cookie", &cookie)
        .send().await.unwrap();
    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["source"], "live");
    assert_eq!(body["data"][0]["id"], "ses_1");
    assert_eq!(body["data"][0]["title"], "会话一");
    device.verify().await;
}
