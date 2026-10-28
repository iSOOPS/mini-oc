//! Task 4：管理端设备审核（approve / reject）与先停用后移除（DELETE 设备）测试。
//!
//! 铺数据方式：approve/reject 走真实 register 流程造 pending（有状态 SB mock）；
//! delete 直接在 mock 的 users JSON 里预置带 device_id 的设备。

mod common;

use common::*;
use mini_oc_core::device::{PortalUser, UsersFile};
use std::sync::{Arc, Mutex};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, Request, ResponseTemplate};

const ADMIN_PASS: &str = "sbpass";

/// SB UsersFile 的共享内存句柄（有状态 mock 用，同 device_register_test.rs）。
type SharedUsers = Arc<Mutex<UsersFile>>;

/// 挂载有状态 SB mock：GET/PUT config.md 读写同一份内存注册表。
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

/// 客户端上报（deviceId 空 → 走 pending 分支），返回响应 JSON。
async fn register_pending(
    base: &str,
    key: &str,
    desc: &str,
    port: u16,
    device_name: &str,
) -> serde_json::Value {
    reqwest::Client::new()
        .post(format!("{base}/api/device-register"))
        .json(&serde_json::json!({
            "key": key,
            "deviceId": "",
            "desc": desc,
            "port": port,
            "pctype": "macos",
            "deviceName": device_name,
            "os": "macos",
        }))
        .send()
        .await
        .unwrap()
        .json::<serde_json::Value>()
        .await
        .unwrap()
}

async fn approve(
    base: &str,
    cookie: &str,
    uid: &str,
    did: &str,
    body: &serde_json::Value,
) -> reqwest::Response {
    reqwest::Client::new()
        .post(format!(
            "{base}/api/admin/users/{uid}/devices/{did}/approve"
        ))
        .header("cookie", cookie)
        .json(body)
        .send()
        .await
        .unwrap()
}

async fn reject(base: &str, cookie: &str, uid: &str, did: &str) -> reqwest::Response {
    reqwest::Client::new()
        .delete(format!("{base}/api/admin/users/{uid}/devices/{did}/reject"))
        .header("cookie", cookie)
        .send()
        .await
        .unwrap()
}

async fn delete_device(base: &str, cookie: &str, uid: &str, did: &str) -> reqwest::Response {
    reqwest::Client::new()
        .delete(format!("{base}/api/admin/users/{uid}/devices/{did}"))
        .header("cookie", cookie)
        .send()
        .await
        .unwrap()
}

/// 第二个用户（无设备），用于跨用户重名测试。
fn second_user() -> PortalUser {
    let mut u = test_user();
    u.id = "200002".into();
    u.name = "alice".into();
    u.key = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb".into();
    u.devices.clear();
    u
}

/// 待审核转正：全字段按 spec 断言（enabled/bound/desc/port/pctype/device_id/device_name）。
#[tokio::test]
async fn approve_moves_pending_to_devices() {
    let sb = MockServer::start().await;
    let mut user = test_user();
    user.devices.clear(); // 排除既有 "macbook-pro" 占用，approve 可用该服务名
    mount_stateful_sb(
        &sb,
        UsersFile {
            version: 2,
            users: vec![user.clone()],
        },
    )
    .await;
    let base = spawn_app(&sb).await;

    let reg = register_pending(&base, &user.key, "客户端描述", 7777, "New-Box").await;
    assert_eq!(reg["status"], "pending");
    let did = reg["deviceId"].as_str().unwrap().to_string();

    let cookie = admin_login(&base).await;
    let resp = approve(
        &base,
        &cookie,
        &user.id,
        &did,
        &serde_json::json!({"name": "macbook-pro", "publicUrl": "https://t.example.com"}),
    )
    .await;
    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();

    let devices = body["user"]["devices"].as_array().unwrap();
    assert_eq!(devices.len(), 1, "转正后 devices 应恰好含该设备");
    let dev = &devices[0];
    assert_eq!(dev["name"], "macbook-pro", "服务名取管理员输入");
    assert_eq!(
        dev["publicUrl"], "https://t.example.com",
        "穿透地址取管理员输入"
    );
    assert_eq!(dev["enabled"], true, "转正设备默认启用");
    assert_eq!(dev["bound"], false, "转正设备未绑定");
    assert_eq!(dev["deviceId"], did.as_str(), "device_id 取上报值");
    assert_eq!(dev["desc"], "客户端描述", "上报 desc 非空则直取");
    assert_eq!(dev["port"], 7777, "port 取上报值");
    assert_eq!(dev["pctype"], "macos", "pctype 取上报值");
    assert_eq!(dev["deviceName"], "New-Box", "主机名取上报值");
    assert_eq!(
        body["user"]["pendingDevices"].as_array().unwrap().len(),
        0,
        "pending 应清空"
    );
}

/// 上报 desc 为空串 → 转正时以 device_name 兜底（满足 UserDevice::validate desc 非空）。
#[tokio::test]
async fn approve_fills_empty_desc_with_device_name() {
    let sb = MockServer::start().await;
    let mut user = test_user();
    user.devices.clear();
    mount_stateful_sb(
        &sb,
        UsersFile {
            version: 2,
            users: vec![user.clone()],
        },
    )
    .await;
    let base = spawn_app(&sb).await;

    let reg = register_pending(&base, &user.key, "", 7777, "Empty-Desc-Box").await;
    assert_eq!(reg["status"], "pending");
    let did = reg["deviceId"].as_str().unwrap().to_string();

    let cookie = admin_login(&base).await;
    let resp = approve(
        &base,
        &cookie,
        &user.id,
        &did,
        &serde_json::json!({"name": "empty-desc-box", "publicUrl": "https://t.example.com"}),
    )
    .await;
    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(
        body["user"]["devices"][0]["desc"], "Empty-Desc-Box",
        "空 desc 转正应以 device_name 兜底"
    );
}

/// 服务名与另一用户设备重名 → 400，且 pending 保留（失败不落盘）。
#[tokio::test]
async fn approve_rejects_duplicate_name_across_users() {
    let sb = MockServer::start().await;
    let user_a = test_user(); // devices[0].name == "macbook-pro"
    let user_b = second_user();
    mount_stateful_sb(
        &sb,
        UsersFile {
            version: 2,
            users: vec![user_a.clone(), user_b.clone()],
        },
    )
    .await;
    let base = spawn_app(&sb).await;

    let reg = register_pending(&base, &user_b.key, "B 的盒子", 7777, "B-Box").await;
    let did = reg["deviceId"].as_str().unwrap().to_string();

    let cookie = admin_login(&base).await;
    let resp = approve(
        &base,
        &cookie,
        &user_b.id,
        &did,
        &serde_json::json!({"name": "macbook-pro", "publicUrl": "https://t.example.com"}),
    )
    .await;
    assert_eq!(resp.status(), 400, "跨用户设备名冲突被拒绝");
    let err: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(err["error"]["code"], "invalid_target");

    let users = admin_users(&base).await;
    let b = users["users"]
        .as_array()
        .unwrap()
        .iter()
        .find(|u| u["id"] == user_b.id.as_str())
        .unwrap();
    assert_eq!(
        b["pendingDevices"].as_array().unwrap().len(),
        1,
        "校验失败不得写回，pending 应保留"
    );
}

/// pending 中无此 device_id → 404。
#[tokio::test]
async fn approve_missing_pending_404() {
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

    let cookie = admin_login(&base).await;
    let resp = approve(
        &base,
        &cookie,
        &user.id,
        "no-such-device-id",
        &serde_json::json!({"name": "some-name", "publicUrl": "https://t.example.com"}),
    )
    .await;
    assert_eq!(resp.status(), 404);
}

/// reject 仅从 pending 移除，devices 不受影响。
#[tokio::test]
async fn reject_removes_pending_only() {
    let sb = MockServer::start().await;
    let user = test_user(); // 保留 devices[0]
    mount_stateful_sb(
        &sb,
        UsersFile {
            version: 2,
            users: vec![user.clone()],
        },
    )
    .await;
    let base = spawn_app(&sb).await;

    // deviceName 与存量 devices[0]（Samuels-MacBook + macos）不同 → 走新建 pending
    let reg = register_pending(&base, &user.key, "待拒设备", 7777, "Reject-Box").await;
    assert_eq!(reg["status"], "pending");
    let did = reg["deviceId"].as_str().unwrap().to_string();

    let cookie = admin_login(&base).await;
    let resp = reject(&base, &cookie, &user.id, &did).await;
    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["ok"], true);

    let users = admin_users(&base).await;
    let u = &users["users"][0];
    assert_eq!(
        u["pendingDevices"].as_array().unwrap().len(),
        0,
        "pending 应被移除"
    );
    assert_eq!(
        u["devices"].as_array().unwrap().len(),
        1,
        "已审核 devices 不受影响"
    );
    assert_eq!(u["devices"][0]["name"], "macbook-pro");
}

/// 删除启用中的设备 → 409 conflict。
#[tokio::test]
async fn delete_enabled_device_conflicts() {
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

    let cookie = admin_login(&base).await;
    let resp = delete_device(
        &base,
        &cookie,
        &user.id,
        "3f2b8a44-1c5d-4e6f-9a8b-7c2d1e0f9a8b",
    )
    .await;
    assert_eq!(resp.status(), 409, "启用中的设备不可删除");
    let err: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(err["error"]["code"], "conflict");

    // 设备未被删除
    let users = admin_users(&base).await;
    assert_eq!(users["users"][0]["devices"].as_array().unwrap().len(), 1);
}

/// 先 PUT enabled:false 停用，再 DELETE → 成功移除。
#[tokio::test]
async fn delete_disabled_device_succeeds() {
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

    // 白名单 PUT：按 deviceId 定位，仅 enabled:false 停用（port 等事实字段不可经 PUT 修改）
    let cookie = admin_login(&base).await;
    let put = reqwest::Client::new()
        .put(format!("{base}/api/admin/users/{}", user.id))
        .header("cookie", &cookie)
        .json(&serde_json::json!({"devices": [{
            "deviceId": "3f2b8a44-1c5d-4e6f-9a8b-7c2d1e0f9a8b",
            "name": "macbook-pro",
            "publicUrl": "https://oc.example.com",
            "enabled": false,
        }]}))
        .send()
        .await
        .unwrap();
    assert_eq!(put.status(), 200, "停用应成功");

    let resp = delete_device(
        &base,
        &cookie,
        &user.id,
        "3f2b8a44-1c5d-4e6f-9a8b-7c2d1e0f9a8b",
    )
    .await;
    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["ok"], true);

    let users = admin_users(&base).await;
    assert_eq!(
        users["users"][0]["devices"].as_array().unwrap().len(),
        0,
        "设备应被移除"
    );
}
