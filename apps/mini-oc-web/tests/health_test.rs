//! Task 5：web 骨架测试——healthz、SB 客户端 UsersFile 语义、断连降级。

mod common;

use common::*;
use mini_oc_core::device::UsersFile;
use mini_oc_web::sb::SbClient;
use wiremock::matchers::{body_string_contains, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

#[tokio::test]
async fn healthz_returns_ok() {
    let sb = MockServer::start().await;
    let base = spawn_app(&sb).await;
    let resp = reqwest::get(format!("{base}/healthz")).await.unwrap();
    assert_eq!(resp.status(), 200);
    assert_eq!(resp.text().await.unwrap(), "ok");
}

#[tokio::test]
async fn sb_get_users_reads_v2_directly() {
    let sb = MockServer::start().await;
    mount_sb_users(&sb, &users_json(vec![test_user()])).await;
    let client = SbClient::new(&sb.uri(), "admin", "sbpass").unwrap();
    let file = client.get_users().await.unwrap();
    assert_eq!(file.version, 2);
    assert_eq!(file.users.len(), 1);
    assert_eq!(file.users[0].id, "100001");
    assert_eq!(file.users[0].devices[0].port, 9464);
}

#[tokio::test]
async fn sb_get_users_migrates_v1_and_writes_back() {
    let sb = MockServer::start().await;
    let v1_body = r#"{"version":1,"users":[{"id":"100001","name":"samuel","key":"testkey1234567890abcdefghij12345","created_at":"t","updated_at":"t","devices":[{"desc":"d","name":"macbook-pro","port":9465,"oc_port":9464,"pctype":"macos","device_name":"","bound":false,"public_url":"https://oc.example.com"}]}]}"#;
    mount_sb_users(&sb, v1_body).await;
    // 捕获迁移写回的 PUT
    Mock::given(method("PUT"))
        .and(path("/.fs/web/opencode/config.md"))
        .and(body_string_contains("\"version\":2"))
        .and(body_string_contains("\"port\": 9464"))
        .respond_with(ResponseTemplate::new(200))
        .expect(1)
        .named("sb-users-put-back")
        .mount(&sb)
        .await;

    let client = SbClient::new(&sb.uri(), "admin", "sbpass").unwrap();
    let file = client.get_users().await.unwrap();
    assert_eq!(file.version, 2, "V1 数据被迁移为 V2");
    assert_eq!(file.users[0].devices[0].port, 9464);
    sb.verify().await; // PUT 恰好被调用 1 次
}

#[tokio::test]
async fn sb_get_users_404_means_empty_registry() {
    let sb = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/.auth"))
        .respond_with(ResponseTemplate::new(200).append_header("set-cookie", "auth_127_0_0_1=jwt; Path=/"))
        .mount(&sb)
        .await;
    Mock::given(method("GET"))
        .and(path("/.fs/web/opencode/config.md"))
        .respond_with(ResponseTemplate::new(404))
        .mount(&sb)
        .await;
    let client = SbClient::new(&sb.uri(), "admin", "sbpass").unwrap();
    let file = client.get_users().await.unwrap();
    assert_eq!(file, UsersFile { version: 2, users: vec![] });
}

#[tokio::test]
async fn sb_unreachable_maps_service_unavailable_not_panic() {
    // SB 指向必然拒绝连接的地址
    let client = SbClient::new("http://127.0.0.1:1", "admin", "sbpass").unwrap();
    let err = client.get_users().await.unwrap_err();
    assert_eq!(err.code().as_str(), "service_unavailable", "got {err}");
}
