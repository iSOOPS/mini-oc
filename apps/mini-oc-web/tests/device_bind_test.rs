//! Task 7：/api/user/info 下发 + /api/device-bind 绑定测试。

mod common;

use common::*;
use mini_oc_core::device::{PortalUser, UserDevice};
use wiremock::matchers::{body_string_contains, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

/// 构造停用设备（enabled=false）的测试用户。
fn disabled_device_user() -> PortalUser {
    let user = test_user();
    PortalUser {
        devices: vec![UserDevice {
            enabled: false,
            ..user.devices[0].clone()
        }],
        ..user
    }
}

/// 登录并返回会话 cookie。
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

#[tokio::test]
async fn disabled_device_hidden_from_user_info() {
    let sb = MockServer::start().await;
    let user = disabled_device_user();
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
    let devices = body["devices"].as_array().expect("devices 数组");
    assert!(
        devices.iter().all(|d| d["name"] != "macbook-pro"),
        "停用设备不应出现在 user/info 下发清单中"
    );
    assert!(devices.is_empty(), "仅有的设备已停用，清单应为空");
}

#[tokio::test]
async fn disabled_device_bind_404() {
    let sb = MockServer::start().await;
    let user = disabled_device_user();
    mount_sb_users(&sb, &users_json(vec![user.clone()])).await;
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
    assert_eq!(resp.status(), 404);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["error"]["code"], "not_found");
    assert!(
        body["error"]["message"]
            .as_str()
            .unwrap()
            .contains("设备 macbook-pro 不存在"),
        "message: {}",
        body["error"]["message"]
    );
}

#[tokio::test]
async fn disabled_device_jump_status_detail_404() {
    let sb = MockServer::start().await;
    // 设备在线：若实现遗漏过滤，探测会成功——保证测试能抓住回归
    let device_server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/info"))
        .respond_with(ResponseTemplate::new(200).set_body_json(
            serde_json::json!({"version": "2.0.32", "pid": 1, "urls": [], "paths": {"tmp": "/t"}}),
        ))
        .mount(&device_server)
        .await;
    let mut user = disabled_device_user();
    user.devices[0].public_url = device_server.uri();
    mount_sb_users(&sb, &users_json(vec![user.clone()])).await;
    let base = spawn_app(&sb).await;

    let cookie = login_cookie(&base, &user.key).await;

    for path in [
        "/api/device-status/macbook-pro",
        "/api/devices/macbook-pro/projects",
        "/api/devices/macbook-pro/sessions",
        "/api/devices/macbook-pro/jump",
        "/api/devices/macbook-pro/detail",
    ] {
        let resp = reqwest::Client::new()
            .get(format!("{base}{path}"))
            .header("cookie", &cookie)
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), 404, "路径 {path} 应对停用设备返回 404");
        let body: serde_json::Value = resp.json().await.unwrap();
        assert_eq!(body["error"]["code"], "not_found", "路径 {path}");
    }

    // jump?format=json 同样 404（不泄露 connect 链接）
    let resp = reqwest::Client::new()
        .get(format!("{base}/api/devices/macbook-pro/jump?format=json"))
        .header("cookie", &cookie)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 404);
}
