//! Task 2：账户模块测试（wiremock 伪 mini-oc-web）。
//!
//! 注意：MINI_OC_HOME 是进程级 env——所有测试经全局 Mutex 串行，避免并行竞态。

use mini_oc_app::account::{bind_device, login, logout};
use std::sync::{Mutex, MutexGuard};
use wiremock::matchers::{body_partial_json, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

/// 进程级串行锁（env 竞态防护）。
static ENV_LOCK: Mutex<()> = Mutex::new(());
struct Guard(MutexGuard<'static, ()>);
impl Guard {
    fn acquire() -> Self {
        Self(ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner()))
    }
}
impl Drop for Guard {
    fn drop(&mut self) {
        std::env::remove_var("MINI_OC_HOME");
    }
}

fn user_info_response() -> serde_json::Value {
    serde_json::json!({
        "id": "100001",
        "name": "samuel",
        "key": "k123456789012345678901234567890a",
        "devices": [
            {"desc": "d", "name": "macbook-pro", "port": 9464, "pctype": "macos",
             "deviceName": "", "bound": false, "publicUrl": "https://oc.example.com"}
        ]
    })
}

/// 串行 + 独立目录的测试环境。
async fn setup() -> (MockServer, Guard) {
    let guard = Guard::acquire();
    let dir = tempfile::tempdir().unwrap();
    std::env::set_var("MINI_OC_HOME", dir.path());
    let server = MockServer::start().await;
    (server, guard)
}

#[tokio::test]
async fn login_ok_stores_user_and_returns_devices() {
    let (server, _guard) = setup().await;
    Mock::given(method("POST"))
        .and(path("/api/user/info"))
        .and(body_partial_json(serde_json::json!({"key": "k123456789012345678901234567890a"})))
        .respond_with(ResponseTemplate::new(200).set_body_json(user_info_response()))
        .expect(1)
        .mount(&server)
        .await;

    let info = login(&server.uri(), "k123456789012345678901234567890a").await.unwrap();
    assert_eq!(info.id, "100001");
    assert_eq!(info.name, "samuel");
    assert_eq!(info.devices.len(), 1);
    assert_eq!(info.devices[0].name, "macbook-pro");
    assert_eq!(info.devices[0].port, 9464);
    server.verify().await;
    // 用户已存入本地配置
    let cfg = mini_oc_app::config::load_config();
    let user = cfg.user.expect("登录后 user 已持久化");
    assert_eq!(user.id, "100001");
    assert_eq!(cfg.server_url, server.uri());
}

#[tokio::test]
async fn login_failure_keeps_existing_config_untouched() {
    let (server, _guard) = setup().await;
    // 预置已登录状态
    mini_oc_app::config::save_config(&mini_oc_app::config::ClientConfig {
        server_url: "https://old.example.com".into(),
        user: Some(mini_oc_app::config::StoredUser {
            id: "100000".into(),
            name: "old".into(),
            key: "old".repeat(8),
        }),
        ..Default::default()
    })
    .unwrap();

    Mock::given(method("POST"))
        .and(path("/api/user/info"))
        .respond_with(ResponseTemplate::new(401).set_body_json(
            serde_json::json!({"error": {"code": "unauthorized", "message": "密钥错误"}}),
        ))
        .mount(&server)
        .await;

    let err = login(&server.uri(), "wrongkey").await.unwrap_err();
    assert!(err.contains("密钥") || err.contains("401"), "错误信息: {err}");
    // 旧配置未被污染
    let cfg = mini_oc_app::config::load_config();
    assert_eq!(cfg.server_url, "https://old.example.com", "登录失败不改配置");
    assert_eq!(cfg.user.unwrap().id, "100000");
}

#[tokio::test]
async fn bind_device_success_writes_bound_device() {
    let (server, _guard) = setup().await;
    mini_oc_app::config::save_config(&mini_oc_app::config::ClientConfig {
        server_url: server.uri(),
        user: Some(mini_oc_app::config::StoredUser {
            id: "100001".into(),
            name: "samuel".into(),
            key: "k123456789012345678901234567890a".into(),
        }),
        ..Default::default()
    })
    .unwrap();

    Mock::given(method("POST"))
        .and(path("/api/device-bind"))
        .and(body_partial_json(serde_json::json!({
            "key": "k123456789012345678901234567890a",
            "userId": "100001",
            "name": "macbook-pro",
            "bound": true,
            "os": "macos"
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"ok": true})))
        .expect(1)
        .mount(&server)
        .await;

    bind_device("macbook-pro", "macos", "macos").await.unwrap();
    server.verify().await;
    assert_eq!(
        mini_oc_app::config::load_config().bound_device.as_deref(),
        Some("macbook-pro")
    );
}

#[tokio::test]
async fn bind_device_os_mismatch_returns_clear_error() {
    let (server, _guard) = setup().await;
    mini_oc_app::config::save_config(&mini_oc_app::config::ClientConfig {
        server_url: server.uri(),
        user: Some(mini_oc_app::config::StoredUser {
            id: "100001".into(),
            name: "samuel".into(),
            key: "k123456789012345678901234567890a".into(),
        }),
        ..Default::default()
    })
    .unwrap();
    Mock::given(method("POST"))
        .and(path("/api/device-bind"))
        .respond_with(ResponseTemplate::new(403).set_body_json(serde_json::json!({
            "error": {"code": "forbidden", "message": "平台不匹配：设备登记为 macos，客户端上报为 windows"}
        })))
        .mount(&server)
        .await;

    let err = bind_device("macbook-pro", "macos", "windows").await.unwrap_err();
    assert!(err.contains("平台不匹配"), "403 文案需含平台不匹配语义: {err}");
}

#[test]
fn logout_clears_user_keeps_server() {
    let _guard = Guard::acquire();
    let dir = tempfile::tempdir().unwrap();
    std::env::set_var("MINI_OC_HOME", dir.path());
    mini_oc_app::config::save_config(&mini_oc_app::config::ClientConfig {
        server_url: "https://oc.example.com".into(),
        user: Some(mini_oc_app::config::StoredUser {
            id: "1".repeat(6),
            name: "x".into(),
            key: "k".repeat(32),
        }),
        bound_device: Some("dev".into()),
        ..Default::default()
    })
    .unwrap();
    logout().unwrap();
    let cfg = mini_oc_app::config::load_config();
    assert!(cfg.user.is_none());
    assert!(cfg.bound_device.is_none());
    assert_eq!(cfg.server_url, "https://oc.example.com", "服务器地址保留");
    assert_eq!(cfg.port, 9464, "端口偏好保留");
}
