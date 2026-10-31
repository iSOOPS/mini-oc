//! I12：设置页进入时的只读上报状态查询（query_registration）。
//!
//! 场景：`registrationProvider` 是内存态，App 重启后丢失 → 设置页永远显示
//! 「尚未上报」，即使设备早已上报/批准。需求：进入设置页时调 web 端接口
//! 获取真实上报情况。
//!
//! 契约（与 report_registration 的 ReportOutcome 同构，便于 UI 复用）：
//! - 拉 `/api/user/info`（无副作用：不写 config、不触发上报）；
//! - 匹配顺序：device_id（config 持久化的）→ device_name+pctype（本机）；
//! - 命中且 bound → status="approved", bound=true；
//! - 命中未 bound → status="approved", bound=false（UI 提示可重试绑定）；
//! - 未命中（待审核或不曾上报）→ status="pending"（服务端清单不下发 pending
//!   明细，无法区分「待审核」与「未上报」——统一显示待确认文案）；
//! - 网络失败 → ok=false（UI 保留上次状态，不误报）。
//!
//! 注意：MINI_OC_HOME 是进程级 env——经全局 Mutex 串行（同 register_test）。

use rust_lib_mini_oc::api::query_registration;
use serde_json::json;
use std::sync::{Mutex, MutexGuard};
use wiremock::matchers::{body_json, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

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

/// 串行测试环境：独立 MINI_OC_HOME + mock server；写指定 config。
/// 返回的 TempDir 随 Guard 存活（目录在测试结束前不删除）。
async fn setup_with_config(
    extra: serde_json::Value,
) -> (MockServer, Guard, tempfile::TempDir) {
    let guard = Guard::acquire();
    let dir = tempfile::tempdir().unwrap();
    std::env::set_var("MINI_OC_HOME", dir.path());
    let server = MockServer::start().await;
    let mut cfg = json!({
        "server_url": server.uri(),
        "user": {"id": "667004", "name": "Samuel", "key": "K".repeat(32)},
        "port": 9464
    });
    let obj = cfg.as_object_mut().unwrap();
    if let Some(extra) = extra.as_object() {
        for (k, v) in extra {
            obj.insert(k.clone(), v.clone());
        }
    }
    std::fs::write(dir.path().join("config.json"), cfg.to_string()).unwrap();
    (server, guard, dir)
}

async fn mock_user_info(server: &MockServer, devices: serde_json::Value) {
    Mock::given(method("POST"))
        .and(path("/api/user/info"))
        .and(body_json(json!({"key": "K".repeat(32)})))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id": "667004", "name": "Samuel", "devices": devices
        })))
        .mount(server)
        .await;
}

#[tokio::test]
async fn query_finds_bound_device_by_id() {
    let (server, _g, _dir) = setup_with_config(json!({
        "device_id": "dev-123", "bound_device": "macbook-pro"
    }))
    .await;

    mock_user_info(&server, json!([{
        "name": "macbook-pro", "port": 9464, "pctype": "macos", "bound": true,
        "deviceName": "MacBook-Pro", "publicUrl": "https://oc.example.com",
        "deviceId": "dev-123", "enabled": true
    }]))
    .await;

    let out = query_registration().await;
    assert!(out.ok, "查询成功: {:?}", out.error);
    assert_eq!(out.status, "approved");
    assert!(out.bound, "清单中 bound=true → 已绑定");
    assert_eq!(out.device_id, "dev-123");
}

#[tokio::test]
async fn query_finds_unbound_device_by_hostname() {
    let (server, _g, _dir) = setup_with_config(json!({})).await;

    // 无 device_id（老配置）：按 主机名+pctype 匹配
    let host = whoami::devicename();
    mock_user_info(&server, json!([{
        "name": "macbook-pro", "port": 9464, "pctype": std::env::consts::OS,
        "bound": false, "deviceName": host, "publicUrl": "https://oc.example.com"
    }]))
    .await;

    let out = query_registration().await;
    assert!(out.ok);
    assert_eq!(out.status, "approved", "命中清单（未绑定）");
    assert!(!out.bound);
}

#[tokio::test]
async fn query_missing_device_reports_pending() {
    let (server, _g, _dir) = setup_with_config(json!({})).await;

    mock_user_info(&server, json!([])).await; // 清单为空（待审核或未上报）

    let out = query_registration().await;
    assert!(out.ok);
    assert_eq!(out.status, "pending", "清单未命中 → 待确认（无法区分待审/未报）");
}

#[tokio::test]
async fn query_network_error_returns_ok_false() {
    // 不起 mock server，config 指向必失败地址（port 1 连接拒绝）。
    // guard 须持有到 query 完成后（提前 drop 会让并行测试改写 MINI_OC_HOME）。
    let guard = Guard::acquire();
    let dir = tempfile::tempdir().unwrap();
    std::env::set_var("MINI_OC_HOME", dir.path());
    let cfg = json!({
        "server_url": "http://127.0.0.1:1",
        "user": {"id": "667004", "name": "Samuel", "key": "K".repeat(32)},
        "port": 9464
    });
    std::fs::write(dir.path().join("config.json"), cfg.to_string()).unwrap();

    let out = query_registration().await;
    drop(guard);

    assert!(!out.ok, "网络失败 → ok=false（UI 不误报已上报）");
    assert!(out.error.is_some());
}

#[tokio::test]
async fn query_requires_login() {
    let guard = Guard::acquire();
    let dir = tempfile::tempdir().unwrap();
    std::env::set_var("MINI_OC_HOME", dir.path());
    std::fs::write(
        dir.path().join("config.json"),
        json!({"server_url": "http://127.0.0.1:8100", "port": 9464}).to_string(),
    )
    .unwrap();

    let out = query_registration().await;
    drop(guard);

    assert!(!out.ok);
}
