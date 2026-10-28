//! Task 3：POST /api/device-register 设备上报注册测试（四态匹配 + 校验）。
//!
//! 匹配顺序（spec 定夺版）：
//! ① deviceId 非空查 devices → ② device_name+pctype 查 devices（存量回填）
//! → ③ device_name+pctype 查 pending_devices → ④ 新建 pending。

mod common;

use common::*;
use mini_oc_core::device::{PendingDevice, UsersFile};
use std::sync::{Arc, Mutex};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, Request, ResponseTemplate};

const ADMIN_PASS: &str = "sbpass";

/// SB UsersFile 的共享内存句柄（有状态 mock 用）。
type SharedUsers = Arc<Mutex<UsersFile>>;

/// 挂载有状态 SB mock：GET/PUT config.md 读写同一份内存注册表，
/// 使「register 写回后 admin 查询可见」走真实读取路径（web → SB）。
async fn mount_stateful_sb(sb: &MockServer, users: UsersFile) -> SharedUsers {
    let shared: SharedUsers = Arc::new(Mutex::new(users));
    let reader = shared.clone();
    Mock::given(method("GET"))
        .and(path("/.fs/web/opencode/config.md"))
        .respond_with(move |_req: &Request| {
            let file = reader.lock().unwrap();
            ResponseTemplate::new(200).set_body_string(serde_json::to_string(&*file).unwrap())
        })
        .named("sb-users-get")
        .mount(sb)
        .await;
    let writer = shared.clone();
    Mock::given(method("PUT"))
        .and(path("/.fs/web/opencode/config.md"))
        .respond_with(move |req: &Request| {
            let file: UsersFile = req.body_json().expect("PUT body 须为合法 UsersFile");
            *writer.lock().unwrap() = file;
            ResponseTemplate::new(200)
        })
        .named("sb-users-put")
        .mount(sb)
        .await;
    shared
}

async fn admin_login(base: &str) -> String {
    reqwest::Client::new()
        .post(format!("{base}/api/admin/login"))
        .json(&serde_json::json!({"password": ADMIN_PASS}))
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

/// 管理员视角读注册表（真实读取路径）。
async fn admin_users(base: &str) -> serde_json::Value {
    let cookie = admin_login(base).await;
    reqwest::Client::new()
        .get(format!("{base}/api/admin/users"))
        .header("cookie", &cookie)
        .send()
        .await
        .unwrap()
        .json::<serde_json::Value>()
        .await
        .unwrap()
}

/// 构造上报请求体（wire 格式见 spec）。
#[allow(clippy::too_many_arguments)]
fn reg_body(
    key: &str,
    device_id: &str,
    desc: &str,
    port: u16,
    pctype: &str,
    device_name: &str,
    os: &str,
) -> serde_json::Value {
    serde_json::json!({
        "key": key,
        "deviceId": device_id,
        "desc": desc,
        "port": port,
        "pctype": pctype,
        "deviceName": device_name,
        "os": os,
    })
}

async fn register(base: &str, body: &serde_json::Value) -> reqwest::Response {
    reqwest::Client::new()
        .post(format!("{base}/api/device-register"))
        .json(body)
        .send()
        .await
        .unwrap()
}

/// ① deviceId 命中 devices → approved，port/desc/deviceName/pctype 被更新，enabled 不变。
#[tokio::test]
async fn register_updates_facts_by_device_id() {
    let sb = MockServer::start().await;
    let mut user = test_user();
    user.devices[0].device_id = "3f2b8a44-1c5d-4e6f-9a8b-7c2d1e0f9a8b".into();
    mount_stateful_sb(
        &sb,
        UsersFile {
            version: 2,
            users: vec![user.clone()],
        },
    )
    .await;
    let base = spawn_app(&sb).await;

    let resp = register(
        &base,
        &reg_body(
            &user.key,
            "3f2b8a44-1c5d-4e6f-9a8b-7c2d1e0f9a8b",
            "客户端上报描述",
            9999,
            "macos",
            "Renamed-MacBook",
            "macos",
        ),
    )
    .await;
    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["status"], "approved");
    assert_eq!(body["deviceId"], "3f2b8a44-1c5d-4e6f-9a8b-7c2d1e0f9a8b");
    assert_eq!(body["enabled"], true, "approved 态返回设备实际 enabled");

    // admin 视角核对事实字段更新、权限字段不动
    let users = admin_users(&base).await;
    let dev = &users["users"][0]["devices"][0];
    assert_eq!(dev["port"], 9999, "port 应被上报更新");
    assert_eq!(dev["desc"], "客户端上报描述", "desc 归客户端");
    assert_eq!(dev["deviceName"], "Renamed-MacBook");
    assert_eq!(dev["enabled"], true, "register 不得改动 enabled");
    assert_eq!(dev["name"], "macbook-pro", "rathole 服务名不归客户端");
    assert_eq!(
        dev["publicUrl"], "https://oc.example.com",
        "穿透地址不归客户端"
    );
}

/// ② deviceId 空 + name+pctype 命中存量 → 回填 deviceId，返回 approved（不是 pending）。
#[tokio::test]
async fn register_backfills_existing_device() {
    let sb = MockServer::start().await;
    let user = test_user(); // devices[0]: Samuels-MacBook + macos，device_id 为空
    mount_stateful_sb(
        &sb,
        UsersFile {
            version: 2,
            users: vec![user.clone()],
        },
    )
    .await;
    let base = spawn_app(&sb).await;

    let resp = register(
        &base,
        &reg_body(
            &user.key,
            "",
            "升级上报",
            9464,
            "macos",
            "Samuels-MacBook",
            "macos",
        ),
    )
    .await;
    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(
        body["status"], "approved",
        "存量回填应直接 approved 而非 pending"
    );
    let device_id = body["deviceId"].as_str().unwrap().to_string();
    assert!(!device_id.is_empty(), "应下发非空 deviceId");

    let users = admin_users(&base).await;
    let dev = &users["users"][0]["devices"][0];
    assert_eq!(
        dev["deviceId"],
        device_id.as_str(),
        "deviceId 应写回存量设备"
    );
    assert_eq!(dev["enabled"], true);
}

/// ④ 全新设备 → 生成 deviceId 入 pending_devices，返回 pending + enabled:false。
#[tokio::test]
async fn register_new_device_goes_pending() {
    let sb = MockServer::start().await;
    let user = test_user();
    mount_stateful_sb(
        &sb,
        UsersFile {
            version: 2,
            users: vec![user.clone()],
        },
    )
    .await;
    let base = spawn_app(&sb).await;

    let resp = register(
        &base,
        &reg_body(
            &user.key,
            "",
            "新盒子",
            8080,
            "macos",
            "Brand-New-Box",
            "macos",
        ),
    )
    .await;
    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["status"], "pending");
    let device_id = body["deviceId"].as_str().unwrap().to_string();
    assert!(!device_id.is_empty());
    assert_eq!(body["enabled"], false, "pending 态 enabled 恒 false");

    let users = admin_users(&base).await;
    let pending = users["users"][0]["pendingDevices"].as_array().unwrap();
    assert_eq!(pending.len(), 1, "应恰好新增一条待审核");
    assert_eq!(pending[0]["deviceId"], device_id.as_str());
    assert_eq!(pending[0]["port"], 8080);
    assert_eq!(pending[0]["pctype"], "macos");
    assert_eq!(pending[0]["deviceName"], "Brand-New-Box");
    assert_eq!(pending[0]["desc"], "新盒子");
    assert!(
        !pending[0]["reportedAt"]
            .as_str()
            .unwrap_or_default()
            .is_empty(),
        "reportedAt 应有值"
    );
    // 已审核 devices 不受影响
    assert_eq!(users["users"][0]["devices"].as_array().unwrap().len(), 1);
}

/// ③ 同特征 pending 命中 → 更新 desc/port/reported_at，deviceId 复用，不新增记录。
#[tokio::test]
async fn register_pending_is_idempotent() {
    let sb = MockServer::start().await;
    let mut user = test_user();
    user.devices.clear(); // 排除 ② 存量回填干扰，专注 pending 匹配
    let known_id = "8c3a1f2b-4d5e-4f6a-9b8c-1d2e3f4a5b6c".to_string();
    let key = user.key.clone();
    user.pending_devices = vec![PendingDevice {
        device_id: known_id.clone(),
        desc: "旧描述".into(),
        port: 7000,
        pctype: "macos".into(),
        device_name: "Samuels-MacBook".into(),
        reported_at: "2026-10-01T00:00:00Z".into(),
    }];
    mount_stateful_sb(
        &sb,
        UsersFile {
            version: 2,
            users: vec![user],
        },
    )
    .await;
    let base = spawn_app(&sb).await;

    // 第一次上报：deviceId 为空，按 name+pctype 命中 pending
    let first = register(
        &base,
        &reg_body(
            &key,
            "",
            "第一次上报",
            8888,
            "macos",
            "Samuels-MacBook",
            "macos",
        ),
    )
    .await
    .json::<serde_json::Value>()
    .await
    .unwrap();
    assert_eq!(first["status"], "pending");
    assert_eq!(first["deviceId"], known_id.as_str(), "应复用既有 deviceId");

    // 第二次上报（客户端已持久化 deviceId 的真实行为）：仍命中同一条
    let second = register(
        &base,
        &reg_body(
            &key,
            &known_id,
            "第二次上报",
            9999,
            "macos",
            "Samuels-MacBook",
            "macos",
        ),
    )
    .await
    .json::<serde_json::Value>()
    .await
    .unwrap();
    assert_eq!(second["status"], "pending");
    assert_eq!(second["deviceId"], known_id.as_str());

    let users = admin_users(&base).await;
    let pending = users["users"][0]["pendingDevices"].as_array().unwrap();
    assert_eq!(pending.len(), 1, "重复上报不得新增 pending 记录");
    assert_eq!(pending[0]["port"], 9999, "port 应被最新上报更新");
    assert_eq!(pending[0]["desc"], "第二次上报");
}

/// 空 desc 上报不得清空存量 desc。
///
/// `UserDevice::validate` 要求 desc 1-64 非空：客户端省略 desc 上报已 approved
/// 设备时若以空串覆盖，后续 admin PUT 编辑该用户会永远 400（desc 在 admin
/// 侧不可修复）。desc 归客户端，但空串仅表示「本次未上报」，不是删除。
#[tokio::test]
async fn register_empty_desc_keeps_existing_desc() {
    let sb = MockServer::start().await;
    let mut user = test_user();
    user.devices[0].desc = "工作机".into();
    user.devices[0].device_id = "3f2b8a44-1c5d-4e6f-9a8b-7c2d1e0f9a8b".into();
    mount_stateful_sb(
        &sb,
        UsersFile {
            version: 2,
            users: vec![user.clone()],
        },
    )
    .await;
    let base = spawn_app(&sb).await;

    // 分支①：deviceId 命中 devices，客户端省略 desc（serde default → 空串）
    let resp = register(
        &base,
        &reg_body(
            &user.key,
            "3f2b8a44-1c5d-4e6f-9a8b-7c2d1e0f9a8b",
            "",
            9999,
            "macos",
            "Renamed-MacBook",
            "macos",
        ),
    )
    .await;
    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["status"], "approved");

    let users = admin_users(&base).await;
    let dev = &users["users"][0]["devices"][0];
    assert_eq!(dev["desc"], "工作机", "空 desc 不得清空存量 desc");
    assert_eq!(dev["port"], 9999, "其余事实字段照常更新");
    assert_eq!(dev["deviceName"], "Renamed-MacBook");

    // 分支②：deviceId 空 + name+pctype 命中存量回填，同样保留 desc
    let resp = register(
        &base,
        &reg_body(&user.key, "", "", 9555, "macos", "Renamed-MacBook", "macos"),
    )
    .await;
    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["status"], "approved", "存量回填应 approved");

    let users = admin_users(&base).await;
    let dev = &users["users"][0]["devices"][0];
    assert_eq!(dev["desc"], "工作机", "分支②同样不得清空 desc");
    assert_eq!(dev["port"], 9555);
}

/// ⑤ os 与 pctype 不一致 → 400（同 device-bind 的平台一致性校验）。
#[tokio::test]
async fn register_rejects_os_mismatch() {
    let sb = MockServer::start().await;
    let user = test_user();
    mount_stateful_sb(
        &sb,
        UsersFile {
            version: 2,
            users: vec![user.clone()],
        },
    )
    .await;
    let base = spawn_app(&sb).await;

    let resp = register(
        &base,
        &reg_body(
            &user.key,
            "",
            "x",
            9464,
            "macos",
            "Samuels-MacBook",
            "windows",
        ),
    )
    .await;
    assert_eq!(resp.status(), 400);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["error"]["code"], "invalid_target");
}

/// ⑥ 错误 key → 401。
#[tokio::test]
async fn register_wrong_key_401() {
    let sb = MockServer::start().await;
    mount_stateful_sb(
        &sb,
        UsersFile {
            version: 2,
            users: vec![test_user()],
        },
    )
    .await;
    let base = spawn_app(&sb).await;

    let resp = register(
        &base,
        &reg_body(
            "wrongkey_wrongkey_wrongkey_wrong",
            "",
            "x",
            9464,
            "macos",
            "Samuels-MacBook",
            "macos",
        ),
    )
    .await;
    assert_eq!(resp.status(), 401);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["error"]["code"], "unauthorized");
}
