//! Task 2：账户模块测试（wiremock 伪 mini-oc-web）。
//!
//! 注意：MINI_OC_HOME 是进程级 env——所有测试经全局 Mutex 串行，避免并行竞态。

use rust_lib_mini_oc::account::{bind_device, login, logout};
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
             "deviceName": "", "bound": false, "publicUrl": "http://127.0.0.1:9469"}
        ]
    })
}

/// 回归（I 修复）：服务端下发 camelCase publicUrl，客户端 DeviceBrief 必须正确
/// 反序列化。曾缺失 #[serde(rename = "publicUrl")] → 永远落 default 空串 →
/// rathole_toml 校验「public_url 非法」→ 启动云服务必失败。
#[tokio::test]
async fn login_devices_carry_public_url() {
    let (server, _guard) = setup().await;
    Mock::given(method("POST"))
        .and(path("/api/user/info"))
        .respond_with(ResponseTemplate::new(200).set_body_json(user_info_response()))
        .mount(&server)
        .await;

    let info = login(&server.uri(), "k123456789012345678901234567890a").await.unwrap();
    let dev = &info.devices[0];
    assert_eq!(
        dev.public_url, "http://127.0.0.1:9469",
        "publicUrl 必须映射到 public_url（camelCase 契约），空串会导致云服务启动失败"
    );
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
    let cfg = rust_lib_mini_oc::config::load_config();
    let user = cfg.user.expect("登录后 user 已持久化");
    assert_eq!(user.id, "100001");
    assert_eq!(cfg.server_url, server.uri());
}

#[tokio::test]
async fn login_failure_keeps_existing_config_untouched() {
    let (server, _guard) = setup().await;
    // 预置已登录状态
    rust_lib_mini_oc::config::save_config(&rust_lib_mini_oc::config::ClientConfig {
        server_url: "https://old.example.com".into(),
        user: Some(rust_lib_mini_oc::config::StoredUser {
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
    let cfg = rust_lib_mini_oc::config::load_config();
    assert_eq!(cfg.server_url, "https://old.example.com", "登录失败不改配置");
    assert_eq!(cfg.user.unwrap().id, "100000");
}

#[tokio::test]
async fn bind_device_success_writes_bound_device() {
    let (server, _guard) = setup().await;
    rust_lib_mini_oc::config::save_config(&rust_lib_mini_oc::config::ClientConfig {
        server_url: server.uri(),
        user: Some(rust_lib_mini_oc::config::StoredUser {
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
        rust_lib_mini_oc::config::load_config().bound_device.as_deref(),
        Some("macbook-pro")
    );
}

#[tokio::test]
async fn bind_device_os_mismatch_returns_clear_error() {
    let (server, _guard) = setup().await;
    rust_lib_mini_oc::config::save_config(&rust_lib_mini_oc::config::ClientConfig {
        server_url: server.uri(),
        user: Some(rust_lib_mini_oc::config::StoredUser {
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
    rust_lib_mini_oc::config::save_config(&rust_lib_mini_oc::config::ClientConfig {
        server_url: "https://oc.example.com".into(),
        user: Some(rust_lib_mini_oc::config::StoredUser {
            id: "1".repeat(6),
            name: "x".into(),
            key: "k".repeat(32),
        }),
        bound_device: Some("dev".into()),
        ..Default::default()
    })
    .unwrap();
    logout().unwrap();
    let cfg = rust_lib_mini_oc::config::load_config();
    assert!(cfg.user.is_none());
    assert!(cfg.bound_device.is_none());
    assert_eq!(cfg.server_url, "https://oc.example.com", "服务器地址保留");
    assert_eq!(cfg.port, 9464, "端口偏好保留");
}

/// I14：V1 服务端响应（kebab-case 字段 + cloud_ip）→ 客户端必须给出
/// 「服务器版本过旧」的明确错误，而非含糊的「响应解析失败」。
/// 场景：生产 oc.isoops.com 仍是 V1，登录页默认地址连过去必失败。
#[tokio::test]
async fn login_v1_server_reports_clear_error() {
    let (server, _guard) = setup().await;
    // V1 真实响应形态（kebab-case device-name / public-url，含 cloud_ip）
    let v1_body = serde_json::json!({
        "id": "100001",
        "name": "samuel",
        "key": "k123456789012345678901234567890a",
        "cloud_ip": "1.2.3.4",
        "devices": [
            {"desc": "d", "name": "macbook-pro", "port": 9464,
             "device-name": "", "pctype": "macos", "bound": false,
             "public-url": ""}
        ]
    });
    Mock::given(method("POST"))
        .and(path("/api/user/info"))
        .respond_with(ResponseTemplate::new(200).set_body_json(v1_body))
        .mount(&server)
        .await;

    let err = login(&server.uri(), "k123456789012345678901234567890a")
        .await
        .unwrap_err();
    assert!(
        err.contains("服务器版本过旧"),
        "V1 响应应提示版本问题，实际: {err}"
    );
}

/// I16：login 不应无脑清空 bound_device——若清单仍含已绑定设备则保留。
/// 场景：用户已绑定 macbook-pro，重新登录（未勾记住密码/重启后），弹层被取消
/// → UI boundDevice 丢失 → 云服务开关永久置灰。绑定是持久事实，登录不该丢。
#[tokio::test]
async fn login_preserves_bound_device_still_in_list() {
    let (server, _guard) = setup().await;
    Mock::given(method("POST"))
        .and(path("/api/user/info"))
        .respond_with(ResponseTemplate::new(200).set_body_json(user_info_response()))
        .mount(&server)
        .await;

    // 预置已绑定状态（模拟此前会话的绑定结果）
    let home = std::env::var("MINI_OC_HOME").unwrap();
    let cfg_path = std::path::Path::new(&home).join("config.json");
    let mut cfg = rust_lib_mini_oc::config::load_config();
    cfg.bound_device = Some("macbook-pro".into());
    rust_lib_mini_oc::config::save_config(&cfg).unwrap();

    let info = login(&server.uri(), "k123456789012345678901234567890a").await.unwrap();
    assert_eq!(info.devices.len(), 1);

    let after = rust_lib_mini_oc::config::load_config();
    assert_eq!(
        after.bound_device.as_deref(),
        Some("macbook-pro"),
        "清单仍含该设备时登录不得清空绑定"
    );
    let _ = cfg_path;
}
