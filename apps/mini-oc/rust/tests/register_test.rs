//! Task 8：设备上报注册测试（wiremock 伪 /api/device-register）。
//!
//! 覆盖：请求体 camelCase 构造、approved/pending 解析、key 错误透出、
//! report_registration 的 device_id 持久化回写与静默降级。
//!
//! 注意：MINI_OC_HOME 是进程级 env——所有测试经全局 Mutex 串行，避免并行竞态。

use rust_lib_mini_oc::account::{register_device, RegisterBody};
use rust_lib_mini_oc::api::report_registration;
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
        std::env::remove_var("MINI_OC_SERVER_URL");
    }
}

/// 串行 + 独立目录的测试环境。
async fn setup() -> (MockServer, Guard) {
    let guard = Guard::acquire();
    let dir = tempfile::tempdir().unwrap();
    std::env::set_var("MINI_OC_HOME", dir.path());
    let server = MockServer::start().await;
    (server, guard)
}

fn sample_body(device_id: &str) -> RegisterBody {
    RegisterBody {
        key: "k".repeat(32),
        device_id: device_id.into(),
        desc: String::new(),
        port: 9464,
        pctype: "macos".into(),
        device_name: "Samuels-MacBook".into(),
        os: "macos".into(),
    }
}

/// 预置已登录配置（server_url 指向伪服务）。
fn seed_logged_in_config(server_uri: &str, device_id: Option<&str>) {
    rust_lib_mini_oc::config::save_config(&rust_lib_mini_oc::config::ClientConfig {
        server_url: server_uri.into(),
        user: Some(rust_lib_mini_oc::config::StoredUser {
            id: "100001".into(),
            name: "samuel".into(),
            key: "k".repeat(32),
        }),
        device_id: device_id.map(str::to_string),
        ..Default::default()
    })
    .unwrap();
}

#[tokio::test]
async fn register_device_sends_camel_case_body_and_parses_approved() {
    let (server, _guard) = setup().await;
    Mock::given(method("POST"))
        .and(path("/api/device-register"))
        .and(body_partial_json(serde_json::json!({
            "key": "k".repeat(32),
            "deviceId": "",
            "port": 9464,
            "pctype": "macos",
            "deviceName": "Samuels-MacBook",
            "os": "macos"
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "status": "approved", "deviceId": "uuid-1", "enabled": true
        })))
        .expect(1)
        .mount(&server)
        .await;

    let result = register_device(&server.uri(), &sample_body(""))
        .await
        .unwrap();
    assert_eq!(result.status, "approved");
    assert_eq!(result.device_id, "uuid-1");
    assert!(result.enabled, "approved 态 enabled 透传");
    server.verify().await;
}

#[tokio::test]
async fn register_device_parses_pending_with_disabled() {
    let (server, _guard) = setup().await;
    Mock::given(method("POST"))
        .and(path("/api/device-register"))
        .and(body_partial_json(serde_json::json!({
            "deviceId": "uuid-have",
            "deviceName": "Samuels-MacBook",
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "status": "pending", "deviceId": "uuid-have", "enabled": false
        })))
        .expect(1)
        .mount(&server)
        .await;

    let result = register_device(&server.uri(), &sample_body("uuid-have"))
        .await
        .unwrap();
    assert_eq!(result.status, "pending");
    assert_eq!(result.device_id, "uuid-have");
    assert!(!result.enabled, "pending 态 enabled 恒 false（spec 定夺）");
    server.verify().await;
}

#[tokio::test]
async fn register_device_key_error_surfaces_message() {
    let (server, _guard) = setup().await;
    Mock::given(method("POST"))
        .and(path("/api/device-register"))
        .respond_with(ResponseTemplate::new(401).set_body_json(
            serde_json::json!({"error": {"code": "unauthorized", "message": "密钥错误"}}),
        ))
        .mount(&server)
        .await;

    let err = register_device(&server.uri(), &sample_body(""))
        .await
        .unwrap_err();
    assert!(
        err.contains("密钥") || err.contains("401"),
        "错误需含语义: {err}"
    );
}

#[tokio::test]
async fn report_registration_persists_new_device_id() {
    let (server, _guard) = setup().await;
    seed_logged_in_config(&server.uri(), None);
    Mock::given(method("POST"))
        .and(path("/api/device-register"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "status": "approved", "deviceId": "uuid-new", "enabled": true
        })))
        .expect(1)
        .mount(&server)
        .await;

    let out = report_registration().await;
    assert!(out.ok, "上报应成功: {:?}", out.error);
    assert_eq!(out.status, "approved");
    assert_eq!(out.device_id, "uuid-new");
    assert!(out.enabled);
    // deviceId 与 config 不同 → 已原子写回 config.json
    let cfg = rust_lib_mini_oc::config::load_config();
    assert_eq!(
        cfg.device_id.as_deref(),
        Some("uuid-new"),
        "device_id 已持久化"
    );
    assert_eq!(cfg.user.unwrap().name, "samuel", "写回不丢其他字段");
    server.verify().await;
}

#[tokio::test]
async fn report_registration_carries_existing_device_id() {
    let (server, _guard) = setup().await;
    seed_logged_in_config(&server.uri(), Some("uuid-have"));
    Mock::given(method("POST"))
        .and(path("/api/device-register"))
        .and(body_partial_json(
            serde_json::json!({"deviceId": "uuid-have"}),
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "status": "approved", "deviceId": "uuid-have", "enabled": true
        })))
        .expect(1)
        .mount(&server)
        .await;

    let out = report_registration().await;
    assert!(out.ok, "上报应成功: {:?}", out.error);
    assert_eq!(out.device_id, "uuid-have");
    // 幂等：config 仍为原值
    assert_eq!(
        rust_lib_mini_oc::config::load_config().device_id.as_deref(),
        Some("uuid-have")
    );
    server.verify().await;
}

#[tokio::test]
async fn report_registration_reports_effective_port() {
    let (server, _guard) = setup().await;
    seed_logged_in_config(&server.uri(), None);
    // 用户改过端口偏好
    let mut cfg = rust_lib_mini_oc::config::load_config();
    cfg.port = 12345;
    rust_lib_mini_oc::config::save_config(&cfg).unwrap();
    Mock::given(method("POST"))
        .and(path("/api/device-register"))
        .and(body_partial_json(serde_json::json!({"port": 12345})))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "status": "approved", "deviceId": "uuid-p", "enabled": true
        })))
        .expect(1)
        .mount(&server)
        .await;

    let out = report_registration().await;
    assert!(out.ok, "上报应成功: {:?}", out.error);
    server.verify().await;
}

#[tokio::test]
async fn report_registration_not_logged_in_returns_error_not_panic() {
    let (_server, _guard) = setup().await;
    // 空 config（未登录）
    rust_lib_mini_oc::config::save_config(&Default::default()).unwrap();
    let out = report_registration().await;
    assert!(!out.ok, "未登录不应 ok");
    assert!(out.error.is_some(), "错误装进 error 字段");
    assert_eq!(out.status, "");
    assert_eq!(out.device_id, "");
    assert!(!out.enabled);
}

#[tokio::test]
async fn report_registration_server_error_degrades_silently() {
    let (server, _guard) = setup().await;
    seed_logged_in_config(&server.uri(), None);
    Mock::given(method("POST"))
        .and(path("/api/device-register"))
        .respond_with(
            ResponseTemplate::new(500).set_body_json(
                serde_json::json!({"error": {"code": "internal", "message": "boom"}}),
            ),
        )
        .mount(&server)
        .await;

    let out = report_registration().await;
    assert!(!out.ok, "服务端 500 不应 ok");
    assert!(out.error.is_some(), "错误装进 error 字段");
    // 失败不写 device_id
    assert_eq!(rust_lib_mini_oc::config::load_config().device_id, None);
}

#[tokio::test]
async fn report_registration_empty_device_id_not_persisted() {
    let (server, _guard) = setup().await;
    seed_logged_in_config(&server.uri(), None);
    // 异常响应：approved 但 deviceId 为空（服务端 bug 防御）
    Mock::given(method("POST"))
        .and(path("/api/device-register"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "status": "approved", "deviceId": "", "enabled": true
        })))
        .mount(&server)
        .await;

    let out = report_registration().await;
    assert!(out.ok);
    assert_eq!(out.device_id, "");
    // 空 deviceId 不落盘（避免 Some("") 污染 config）
    assert_eq!(rust_lib_mini_oc::config::load_config().device_id, None);
}
