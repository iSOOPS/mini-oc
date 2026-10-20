//! Task 7：/api/user/info 下发 + /api/device-bind 绑定测试。

mod common;

use common::*;
use mini_oc_core::device::PortalUser;
use wiremock::matchers::{body_string_contains, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

#[tokio::test]
async fn user_info_returns_full_config_without_sb_block() {
    let sb = MockServer::start().await;
    let user = test_user();
    mount_sb_users(&sb, &users_json(vec![user.clone()])).await;
    let base = spawn_app(&sb).await;

    let resp = reqwest::Client::new()
        .post(format!("{base}/api/user/info"))
        .json(&serde_json::json!({"key": user.key}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["id"], "100001");
    assert_eq!(body["name"], "samuel");
    assert_eq!(body["key"], user.key, "客户端需要 key 派生凭据");
    assert!(body.get("sb").is_none(), "V2 已无用户级 SB 配置");
    assert_eq!(body["devices"][0]["name"], "macbook-pro");
    assert_eq!(body["devices"][0]["port"], 9464);
    assert!(body["devices"][0].get("ocPort").is_none());
}

#[tokio::test]
async fn user_info_wrong_key_401() {
    let sb = MockServer::start().await;
    mount_sb_users(&sb, &users_json(vec![test_user()])).await;
    let base = spawn_app(&sb).await;
    let resp = reqwest::Client::new()
        .post(format!("{base}/api/user/info"))
        .json(&serde_json::json!({"key": "wrong"}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 401);
}

#[tokio::test]
async fn device_bind_writes_back_and_updates_timestamp() {
    let sb = MockServer::start().await;
    let user = test_user();
    mount_sb_users(&sb, &users_json(vec![user.clone()])).await;
    // 捕获写回：bound=true + device_name
    Mock::given(method("PUT"))
        .and(path("/.fs/web/opencode/config.md"))
        .and(body_string_contains("\"bound\": true"))
        .and(body_string_contains("\"deviceName\": \"New-MacBook\""))
        .respond_with(ResponseTemplate::new(200))
        .expect(1)
        .mount(&sb)
        .await;
    let base = spawn_app(&sb).await;

    let resp = reqwest::Client::new()
        .post(format!("{base}/api/device-bind"))
        .json(&serde_json::json!({
            "key": user.key,
            "user_id": user.id,
            "name": "macbook-pro",
            "bound": true,
            "pctype": "macos",
            "deviceName": "New-MacBook",
            "os": "macos"
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["ok"], true);
    sb.verify().await;
}

#[tokio::test]
async fn device_bind_os_mismatch_403() {
    let sb = MockServer::start().await;
    let user = test_user();
    mount_sb_users(&sb, &users_json(vec![user])).await;
    let base = spawn_app(&sb).await;
    let resp = reqwest::Client::new()
        .post(format!("{base}/api/device-bind"))
        .json(&serde_json::json!({
            "key": test_user().key,
            "user_id": "100001",
            "name": "macbook-pro",
            "bound": true,
            "pctype": "macos",
            "deviceName": "Some-PC",
            "os": "windows"   // ← 平台不匹配
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 403);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["error"]["code"], "forbidden");
}

#[tokio::test]
async fn device_bind_cross_user_key_403() {
    let sb = MockServer::start().await;
    let user = test_user();
    mount_sb_users(&sb, &users_json(vec![user.clone()])).await;
    let base = spawn_app(&sb).await;
    // key 正确但声明了别人的 user_id
    let resp = reqwest::Client::new()
        .post(format!("{base}/api/device-bind"))
        .json(&serde_json::json!({
            "key": user.key,
            "user_id": "100999",
            "name": "macbook-pro",
            "bound": true,
            "pctype": "macos",
            "deviceName": "x",
            "os": "macos"
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 403);
}

#[tokio::test]
async fn device_status_deep_probe_by_name() {
    let sb = MockServer::start().await;
    // 设备在线
    let device_server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/info"))
        .respond_with(ResponseTemplate::new(200).set_body_json(
            serde_json::json!({"version": "2.0.32", "pid": 1, "urls": [], "paths": {"tmp": "/t"}}),
        ))
        .mount(&device_server)
        .await;
    let mut user = test_user();
    user.devices[0].public_url = device_server.uri();
    mount_sb_users(&sb, &users_json(vec![user.clone()])).await;
    let base = spawn_app(&sb).await;

    let cookie = reqwest::Client::new()
        .post(format!("{base}/api/login"))
        .json(&serde_json::json!({"key": user.key}))
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
        .to_string();

    // 按名深探测
    let resp = reqwest::Client::new()
        .get(format!("{base}/api/device-status/macbook-pro"))
        .header("cookie", &cookie)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["status"], "online");
    assert_eq!(body["version"], "2.0.32");

    // 不存在的设备
    let resp = reqwest::Client::new()
        .get(format!("{base}/api/device-status/no-such"))
        .header("cookie", &cookie)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 404);
}
