//! Task 9：会话跳转 pair-link 测试。

mod common;

use common::*;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

async fn login_cookie(base: &str) -> String {
    reqwest::Client::new()
        .post(format!("{base}/api/login"))
        .json(&serde_json::json!({"key": test_user().key}))
        .send().await.unwrap()
        .headers().get("set-cookie").unwrap().to_str().unwrap()
        .split(';').next().unwrap().to_string()
}

#[tokio::test]
async fn jump_live_returns_pair_connect_url_json() {
    let sb = MockServer::start().await;
    // 设备端 pair 端点
    let device = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/api/pair"))
        .respond_with(ResponseTemplate::new(200).set_body_json(
            serde_json::json!({"code": "610bsBT_9EqdC", "expires_in": 300}),
        ))
        .expect(1)
        .mount(&device).await;
    let mut user = test_user();
    user.devices[0].public_url = device.uri();
    mount_sb_users(&sb, &users_json(vec![user])).await;
    let base = spawn_app(&sb).await;
    let cookie = login_cookie(&base).await;

    let resp = reqwest::Client::new()
        .get(format!("{base}/api/devices/macbook-pro/jump?directory=/Users/x/proj&format=json"))
        .header("cookie", &cookie)
        .send().await.unwrap();
    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(
        body["url"],
        format!("{}/auth/connect/610bsBT_9EqdC", device.uri()),
        "跳转 URL = 隧道/auth/connect/{{code}}"
    );
    assert_eq!(body["expiresIn"], 300);
    device.verify().await;
}

#[tokio::test]
async fn jump_browser_gets_302() {
    let sb = MockServer::start().await;
    let device = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/api/pair"))
        .respond_with(ResponseTemplate::new(200).set_body_json(
            serde_json::json!({"code": "abc123", "expires_in": 300}),
        ))
        .mount(&device).await;
    let mut user = test_user();
    user.devices[0].public_url = device.uri();
    mount_sb_users(&sb, &users_json(vec![user])).await;
    let base = spawn_app(&sb).await;
    let cookie = login_cookie(&base).await;

    let client = reqwest::Client::builder().redirect(reqwest::redirect::Policy::none()).build().unwrap();
    let resp = client
        .get(format!("{base}/api/devices/macbook-pro/jump?directory=/Users/x/proj"))
        .header("cookie", &cookie)
        .send().await.unwrap();
    assert_eq!(resp.status(), 302);
    let loc = resp.headers().get("location").unwrap().to_str().unwrap();
    assert_eq!(loc, format!("{}/auth/connect/abc123", device.uri()));
}

#[tokio::test]
async fn jump_device_offline_502_no_legacy_fallback() {
    let sb = MockServer::start().await;
    let mut user = test_user();
    user.devices[0].public_url = "http://127.0.0.1:1".into();
    mount_sb_users(&sb, &users_json(vec![user])).await;
    let base = spawn_app(&sb).await;
    let cookie = login_cookie(&base).await;

    let resp = reqwest::Client::new()
        .get(format!("{base}/api/devices/macbook-pro/jump?directory=/Users/x/proj&format=json"))
        .header("cookie", &cookie)
        .send().await.unwrap();
    assert_eq!(resp.status(), 502);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["error"]["code"], "device_offline", "无 legacy 回退");
}

#[tokio::test]
async fn jump_pair_url_guard_rejects_foreign_origin() {
    // validate_jump_url 纯函数测试：设备返回的 code 被拼装后必须落在设备隧道 origin 内
    // （防开放重定向三重校验的核心：connect URL 的 origin 必须等于 tunnel base 的 origin）
    let tunnel = mini_oc_core::tunnel::tunnel_base_url("https://oc.example.com", 9464).unwrap();
    assert_eq!(tunnel, "https://oc.example.com/9464");
    let jump = mini_oc_web::jump::validate_jump_url(
        &format!("{tunnel}/auth/connect/abc"),
        &tunnel,
    );
    assert!(jump.is_ok(), "同源 connect URL 合法");
    // 不同 origin 拒绝
    let bad = mini_oc_web::jump::validate_jump_url(
        "https://evil.example.com/auth/connect/abc",
        &tunnel,
    );
    assert!(bad.is_err(), "异源 URL 必须拒绝");
}

#[tokio::test]
async fn jump_with_session_returns_deep_link() {
    let sb = MockServer::start().await;
    let device = MockServer::start().await;
    // pair → code
    Mock::given(method("POST"))
        .and(path("/api/pair"))
        .respond_with(ResponseTemplate::new(200).set_body_json(
            serde_json::json!({"code": "610bsBT_9EqdC", "expires_in": 300}),
        ))
        .expect(1)
        .mount(&device).await;
    // connect → token
    Mock::given(method("GET"))
        .and(path("/auth/connect/610bsBT_9EqdC"))
        .respond_with(ResponseTemplate::new(200).set_body_json(
            serde_json::json!({"token": "1796025749.EKzkuAmKP9wfB4QAWNYZNKlPddvzWv9ruxIYnWJ4ZHg"}),
        ))
        .expect(1)
        .mount(&device).await;

    let mut user = test_user();
    user.devices[0].public_url = device.uri();
    mount_sb_users(&sb, &users_json(vec![user])).await;
    let base = spawn_app(&sb).await;
    let cookie = login_cookie(&base).await;

    let resp = reqwest::Client::new()
        .get(format!(
            "{base}/api/devices/macbook-pro/jump?directory=/Users/x/proj&session=ses_abc123&format=json"
        ))
        .header("cookie", &cookie)
        .send().await.unwrap();
    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    // 深链 = session_deep_link(隧道, session, token)，验证 handler 接线正确
    let expected = mini_oc_web::jump::session_deep_link(
        &device.uri(),
        "ses_abc123",
        "1796025749.EKzkuAmKP9wfB4QAWNYZNKlPddvzWv9ruxIYnWJ4ZHg",
    )
    .unwrap();
    assert_eq!(body["url"], expected);
    assert_eq!(body["expiresIn"], 300);
    device.verify().await;
}

#[tokio::test]
async fn jump_with_session_falls_back_to_connect_on_token_failure() {
    // pair 成功，但 connect 换 token 失败（如 code 被消费）→ 回退 connect 链接（旧行为，至少能登录）
    let sb = MockServer::start().await;
    let device = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/api/pair"))
        .respond_with(ResponseTemplate::new(200).set_body_json(
            serde_json::json!({"code": "610bsBT_9EqdC", "expires_in": 300}),
        ))
        .expect(1)
        .mount(&device).await;
    Mock::given(method("GET"))
        .and(path("/auth/connect/610bsBT_9EqdC"))
        .respond_with(ResponseTemplate::new(401).set_body_json(
            serde_json::json!({"_tag": "UnauthorizedError", "message": "Authentication required"}),
        ))
        .expect(1)
        .mount(&device).await;

    let mut user = test_user();
    user.devices[0].public_url = device.uri();
    mount_sb_users(&sb, &users_json(vec![user])).await;
    let base = spawn_app(&sb).await;
    let cookie = login_cookie(&base).await;

    let resp = reqwest::Client::new()
        .get(format!(
            "{base}/api/devices/macbook-pro/jump?directory=/Users/x/proj&session=ses_abc123&format=json"
        ))
        .header("cookie", &cookie)
        .send().await.unwrap();
    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(
        body["url"],
        format!("{}/auth/connect/610bsBT_9EqdC", device.uri()),
        "token 换取失败应回退 connect 链接"
    );
    device.verify().await;
}

#[tokio::test]
async fn device_detail_returns_no_credentials() {
    let sb = MockServer::start().await;
    let user = test_user();
    mount_sb_users(&sb, &users_json(vec![user])).await;
    let base = spawn_app(&sb).await;
    let cookie = login_cookie(&base).await;

    let resp = reqwest::Client::new()
        .get(format!("{base}/api/devices/macbook-pro/detail"))
        .header("cookie", &cookie)
        .send().await.unwrap();
    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["tunnelBase"], "https://oc.example.com/9464");
    assert_eq!(body["port"], 9464);
    // 安全断言：不得含 key/password 等任何凭据字段
    assert!(body.get("key").is_none());
    assert!(body.get("password").is_none());
    let raw = body.to_string();
    assert!(!raw.contains("testkey"), "响应不得含用户密钥");
}
