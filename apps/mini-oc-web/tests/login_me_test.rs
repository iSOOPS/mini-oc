//! Task 6：登录 + /api/me 探测三态测试。

mod common;

use common::*;
use mini_oc_core::device::UserDevice;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

/// 登录并返回 cookie。
async fn login(base: &str, key: &str) -> reqwest::Response {
    reqwest::Client::new()
        .post(format!("{base}/api/login"))
        .json(&serde_json::json!({"key": key}))
        .send()
        .await
        .unwrap()
}

#[tokio::test]
async fn login_ok_sets_cookie_and_me_returns_devices() {
    let sb = MockServer::start().await;
    let base = spawn_app(&sb).await;

    // 设备探测：wiremock 伪 opencode（带端口形态的 public_url → 原样 base）
    let device_server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/info"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "version": "2.0.32", "pid": 5, "urls": [], "paths": {"tmp": "/t"}
        })))
        .mount(&device_server)
        .await;
    let mut user = test_user();
    user.devices[0].public_url = device_server.uri();
    mount_sb_users(&sb, &users_json(vec![user.clone()])).await;

    let resp = login(&base, &user.key).await;
    assert_eq!(resp.status(), 200);
    let cookie = resp
        .headers()
        .get("set-cookie")
        .unwrap()
        .to_str()
        .unwrap()
        .to_string();
    assert!(
        cookie.starts_with("mini_oc_session="),
        "cookie 名: {cookie}"
    );
    assert!(cookie.contains("HttpOnly"));
    assert!(cookie.contains("SameSite=Lax"));

    let me = reqwest::Client::new()
        .get(format!("{base}/api/me"))
        .header("cookie", cookie)
        .send()
        .await
        .unwrap();
    assert_eq!(me.status(), 200);
    let body: serde_json::Value = me.json().await.unwrap();
    assert_eq!(body["user"]["id"], "100001");
    assert_eq!(body["devices"][0]["name"], "macbook-pro");
    assert_eq!(body["devices"][0]["status"], "online");
    assert_eq!(body["devices"][0]["version"], "2.0.32");
}

use mini_oc_core::device::PortalUser;

#[tokio::test]
async fn login_wrong_key_401_and_rate_limited() {
    let sb = MockServer::start().await;
    mount_sb_users(&sb, &users_json(vec![test_user()])).await;
    let base = spawn_app(&sb).await;

    for i in 0..5 {
        let resp = login(&base, "wrongkey").await;
        assert_eq!(resp.status(), 401, "第 {i} 次错误 key");
        let body: serde_json::Value = resp.json().await.unwrap();
        assert_eq!(body["error"]["code"], "unauthorized");
    }
    // 第 6 次被限流
    let resp = login(&base, "wrongkey").await;
    assert_eq!(resp.status(), 429);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["error"]["code"], "rate_limited");
}

#[tokio::test]
async fn me_without_cookie_401() {
    let sb = MockServer::start().await;
    mount_sb_users(&sb, &users_json(vec![test_user()])).await;
    let base = spawn_app(&sb).await;
    let resp = reqwest::get(format!("{base}/api/me")).await.unwrap();
    assert_eq!(resp.status(), 401);
}

#[tokio::test]
async fn me_device_401_maps_auth_mismatch() {
    let sb = MockServer::start().await;
    let user = test_user();
    let device_server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/info"))
        .respond_with(ResponseTemplate::new(401))
        .mount(&device_server)
        .await;
    mount_sb_users(
        &sb,
        &users_json(vec![PortalUser {
            devices: vec![UserDevice {
                public_url: device_server.uri(),
                ..user.devices[0].clone()
            }],
            ..user
        }]),
    )
    .await;
    let base = spawn_app(&sb).await;
    let cookie = login(&base, &test_user().key)
        .await
        .headers()
        .get("set-cookie")
        .unwrap()
        .to_str()
        .unwrap()
        .split(';')
        .next()
        .unwrap()
        .to_string();

    let me = reqwest::Client::new()
        .get(format!("{base}/api/me"))
        .header("cookie", cookie)
        .send()
        .await
        .unwrap();
    assert_eq!(me.status(), 200);
    let body: serde_json::Value = me.json().await.unwrap();
    assert_eq!(body["devices"][0]["status"], "auth_mismatch");
    assert!(body["devices"][0]["version"].is_null());
}

#[tokio::test]
async fn me_device_unreachable_maps_offline() {
    let sb = MockServer::start().await;
    let mut user = test_user();
    // public_url 指向必然拒绝的地址（带端口形态原样使用）
    user.devices[0].public_url = "http://127.0.0.1:1".into();
    mount_sb_users(&sb, &users_json(vec![user.clone()])).await;
    let base = spawn_app(&sb).await;
    let cookie = login(&base, &user.key)
        .await
        .headers()
        .get("set-cookie")
        .unwrap()
        .to_str()
        .unwrap()
        .split(';')
        .next()
        .unwrap()
        .to_string();

    let me = reqwest::Client::new()
        .get(format!("{base}/api/me"))
        .header("cookie", cookie)
        .send()
        .await
        .unwrap();
    assert_eq!(me.status(), 200);
    let body: serde_json::Value = me.json().await.unwrap();
    assert_eq!(body["devices"][0]["status"], "offline");
}

#[tokio::test]
async fn disabled_device_hidden_from_me_and_not_probed() {
    let sb = MockServer::start().await;
    // 设备在线：若实现遗漏过滤，会误探测该设备并下发——保证测试能抓住两类回归
    let device_server = MockServer::start().await;
    let probe_mock = Mock::given(method("GET"))
        .and(path("/api/info"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "version": "2.0.32", "pid": 5, "urls": [], "paths": {"tmp": "/t"}
        })))
        .expect(0)
        .mount_as_scoped(&device_server)
        .await;
    let mut user = test_user();
    user.devices[0].enabled = false;
    user.devices[0].public_url = device_server.uri();
    mount_sb_users(&sb, &users_json(vec![user.clone()])).await;
    let base = spawn_app(&sb).await;

    let cookie = login(&base, &user.key)
        .await
        .headers()
        .get("set-cookie")
        .unwrap()
        .to_str()
        .unwrap()
        .split(';')
        .next()
        .unwrap()
        .to_string();

    let me = reqwest::Client::new()
        .get(format!("{base}/api/me"))
        .header("cookie", cookie)
        .send()
        .await
        .unwrap();
    assert_eq!(me.status(), 200);
    let body: serde_json::Value = me.json().await.unwrap();
    let devices = body["devices"].as_array().expect("devices 数组");
    assert!(
        devices.iter().all(|d| d["name"] != "macbook-pro"),
        "停用设备不应出现在 /api/me 下发清单中"
    );
    assert!(devices.is_empty(), "仅有的设备已停用，清单应为空");
    drop(probe_mock);
    device_server.verify().await; // scoped mock 期望 0 次命中：停用设备未被探测
}
