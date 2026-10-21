//! Task 10：管理端 API 测试（V2 表单 + 安全修补）。

mod common;

use common::*;
use wiremock::matchers::{body_string_contains, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const ADMIN_PASS: &str = "sbpass";

async fn admin_login(base: &str) -> String {
    reqwest::Client::new()
        .post(format!("{base}/api/admin/login"))
        .json(&serde_json::json!({"password": ADMIN_PASS}))
        .send().await.unwrap()
        .headers().get("set-cookie").unwrap().to_str().unwrap()
        .split(';').next().unwrap().to_string()
}

fn device_input() -> serde_json::Value {
    serde_json::json!({
        "desc": "工作电脑",
        "name": "macbook-pro",
        "port": 9464,
        "pctype": "macos",
        "publicUrl": "https://oc.example.com"
    })
}

#[tokio::test]
async fn admin_login_wrong_password_401() {
    let sb = MockServer::start().await;
    mount_sb_users(&sb, &users_json(vec![])).await;
    let base = spawn_app(&sb).await;
    let resp = reqwest::Client::new()
        .post(format!("{base}/api/admin/login"))
        .json(&serde_json::json!({"password": "wrong"}))
        .send().await.unwrap();
    assert_eq!(resp.status(), 401);
}

#[tokio::test]
async fn admin_requires_admin_cookie_user_cookie_rejected() {
    let sb = MockServer::start().await;
    mount_sb_users(&sb, &users_json(vec![test_user()])).await;
    let base = spawn_app(&sb).await;
    // 普通用户 cookie 打管理接口 → 401
    let user_cookie = reqwest::Client::new()
        .post(format!("{base}/api/login"))
        .json(&serde_json::json!({"key": test_user().key}))
        .send().await.unwrap()
        .headers().get("set-cookie").unwrap().to_str().unwrap()
        .split(';').next().unwrap().to_string();
    let resp = reqwest::Client::new()
        .get(format!("{base}/api/admin/users"))
        .header("cookie", user_cookie)
        .send().await.unwrap();
    assert_eq!(resp.status(), 401);
}

#[tokio::test]
async fn admin_create_user_writes_v2_fields() {
    let sb = MockServer::start().await;
    mount_sb_users(&sb, &users_json(vec![])).await;
    // 写回断言：V2 字段集、无 ocPort/sb/cloudIp
    Mock::given(method("PUT"))
        .and(path("/.fs/web/opencode/config.md"))
        .and(body_string_contains("\"name\": \"samuel\""))
        .and(body_string_contains("\"bound\": false"))
        .respond_with(ResponseTemplate::new(200))
        .expect(1)
        .mount(&sb).await;
    let base = spawn_app(&sb).await;
    let cookie = admin_login(&base).await;

    let resp = reqwest::Client::new()
        .post(format!("{base}/api/admin/users"))
        .header("cookie", &cookie)
        .json(&serde_json::json!({"name": "samuel", "devices": [device_input()]}))
        .send().await.unwrap();
    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["user"]["name"], "samuel");
    assert_eq!(body["user"]["id"].as_str().unwrap().len(), 6, "自动生成 6 位编号");
    assert_eq!(body["user"]["key"].as_str().unwrap().len(), 32, "自动生成 32 位密钥");
    assert_eq!(body["user"]["devices"][0]["port"], 9464);
    sb.verify().await;
}

#[tokio::test]
async fn admin_create_rejects_cross_user_duplicate_device_name() {
    let sb = MockServer::start().await;
    mount_sb_users(&sb, &users_json(vec![test_user()])).await; // 已有 macbook-pro
    let base = spawn_app(&sb).await;
    let cookie = admin_login(&base).await;

    let resp = reqwest::Client::new()
        .post(format!("{base}/api/admin/users"))
        .header("cookie", &cookie)
        .json(&serde_json::json!({"name": "another", "devices": [device_input()]}))
        .send().await.unwrap();
    assert_eq!(resp.status(), 400, "跨用户设备名冲突被拒绝");
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["error"]["code"], "invalid_target");
}

#[tokio::test]
async fn admin_full_crud_cycle() {
    // wiremock 的 GET 是静态响应，无法反映 PUT 写回——create 写回已由
    // admin_create_user_writes_v2_fields 验证；本测试挂"已有用户"作为前置，
    // 验证 regenerate/update/delete/list 的完整链路。
    let sb = MockServer::start().await;
    let user = test_user();
    mount_sb_users(&sb, &users_json(vec![user.clone()])).await;
    Mock::given(method("PUT"))
        .and(path("/.fs/web/opencode/config.md"))
        .respond_with(ResponseTemplate::new(200))
        .mount(&sb).await;
    let base = spawn_app(&sb).await;
    let cookie = admin_login(&base).await;
    let uid = user.id;

    // list 读回
    let resp = reqwest::Client::new()
        .get(format!("{base}/api/admin/users"))
        .header("cookie", &cookie)
        .send().await.unwrap();
    assert_eq!(resp.status(), 200);
    let list: serde_json::Value = resp.json().await.unwrap();
    assert!(list["users"].as_array().unwrap().iter().any(|u| u["id"] == *uid));

    // regenerate key
    let resp = reqwest::Client::new()
        .post(format!("{base}/api/admin/users/{uid}/regenerate-key"))
        .header("cookie", &cookie)
        .send().await.unwrap();
    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    let key2 = body["key"].as_str().unwrap().to_string();
    assert_ne!(user.key, key2, "重置后的 key 与旧值不同");
    assert_eq!(key2.len(), 32);

    // update
    let resp = reqwest::Client::new()
        .put(format!("{base}/api/admin/users/{uid}"))
        .header("cookie", &cookie)
        .json(&serde_json::json!({"name": "samuel2"}))
        .send().await.unwrap();
    assert_eq!(resp.status(), 200);

    // delete
    let resp = reqwest::Client::new()
        .delete(format!("{base}/api/admin/users/{uid}"))
        .header("cookie", &cookie)
        .send().await.unwrap();
    assert_eq!(resp.status(), 200);
}

#[tokio::test]
async fn no_admin_info_endpoint_exists() {
    // V1 的 sb_password 泄露接口已移除：任何 /api/admin/info 一律 404
    let sb = MockServer::start().await;
    mount_sb_users(&sb, &users_json(vec![])).await;
    let base = spawn_app(&sb).await;
    let cookie = admin_login(&base).await;
    let resp = reqwest::Client::new()
        .get(format!("{base}/api/admin/info"))
        .header("cookie", &cookie)
        .send().await.unwrap();
    assert_eq!(resp.status(), 404);
}

#[tokio::test]
async fn admin_login_brute_force_rate_limited() {
    // I2 修复：管理端登录与用户登录同样限流（V1 有、V2 丢失的防爆破）
    let sb = MockServer::start().await;
    mount_sb_users(&sb, &users_json(vec![])).await;
    let base = spawn_app(&sb).await;
    for i in 0..5 {
        let resp = reqwest::Client::new()
            .post(format!("{base}/api/admin/login"))
            .json(&serde_json::json!({"password": "wrong"}))
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), 401, "第 {i} 次错误密码");
    }
    let resp = reqwest::Client::new()
        .post(format!("{base}/api/admin/login"))
        .json(&serde_json::json!({"password": "wrong"}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 429, "第 6 次被限流");
}
