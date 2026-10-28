//! Task 10：管理端 API 测试（V2 表单 + 安全修补）。
//! Task 5：PUT 白名单收敛 + CreateUserBody 去设备（create 只填用户名）。

mod common;

use common::*;
use mini_oc_core::device::UsersFile;
use std::sync::{Arc, Mutex};
use wiremock::matchers::{body_string_contains, method, path};
use wiremock::{Mock, MockServer, Request, ResponseTemplate};

const ADMIN_PASS: &str = "sbpass";

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

#[tokio::test]
async fn admin_login_wrong_password_401() {
    let sb = MockServer::start().await;
    mount_sb_users(&sb, &users_json(vec![])).await;
    let base = spawn_app(&sb).await;
    let resp = reqwest::Client::new()
        .post(format!("{base}/api/admin/login"))
        .json(&serde_json::json!({"password": "wrong"}))
        .send()
        .await
        .unwrap();
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
        .to_string();
    let resp = reqwest::Client::new()
        .get(format!("{base}/api/admin/users"))
        .header("cookie", user_cookie)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 401);
}

#[tokio::test]
async fn admin_create_user_writes_v2_fields() {
    let sb = MockServer::start().await;
    mount_sb_users(&sb, &users_json(vec![])).await;
    // 写回断言：V2 字段集、devices 恒为空数组
    Mock::given(method("PUT"))
        .and(path("/.fs/web/opencode/config.md"))
        .and(body_string_contains("\"name\": \"samuel\""))
        .and(body_string_contains("\"devices\": []"))
        .respond_with(ResponseTemplate::new(200))
        .expect(1)
        .mount(&sb)
        .await;
    let base = spawn_app(&sb).await;
    let cookie = admin_login(&base).await;

    let resp = reqwest::Client::new()
        .post(format!("{base}/api/admin/users"))
        .header("cookie", &cookie)
        .json(&serde_json::json!({"name": "samuel"}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["user"]["name"], "samuel");
    assert_eq!(
        body["user"]["id"].as_str().unwrap().len(),
        6,
        "自动生成 6 位编号"
    );
    assert_eq!(
        body["user"]["key"].as_str().unwrap().len(),
        32,
        "自动生成 32 位密钥"
    );
    assert_eq!(
        body["user"]["devices"].as_array().unwrap().len(),
        0,
        "新建用户 devices 恒为空数组（设备改由客户端上报产生）"
    );
    sb.verify().await;
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
        .mount(&sb)
        .await;
    let base = spawn_app(&sb).await;
    let cookie = admin_login(&base).await;
    let uid = user.id;

    // list 读回
    let resp = reqwest::Client::new()
        .get(format!("{base}/api/admin/users"))
        .header("cookie", &cookie)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let list: serde_json::Value = resp.json().await.unwrap();
    assert!(list["users"]
        .as_array()
        .unwrap()
        .iter()
        .any(|u| u["id"] == *uid));

    // regenerate key
    let resp = reqwest::Client::new()
        .post(format!("{base}/api/admin/users/{uid}/regenerate-key"))
        .header("cookie", &cookie)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    let key2 = body["key"].as_str().unwrap().to_string();
    assert_ne!(user.key, key2, "重置后的 key 与旧值不同");
    assert_eq!(key2.len(), 32);

    // update：顶层改名 + 设备白名单（publicUrl/enabled）
    let resp = reqwest::Client::new()
        .put(format!("{base}/api/admin/users/{uid}"))
        .header("cookie", &cookie)
        .json(&serde_json::json!({"name": "samuel2", "devices": [{
            "name": "macbook-pro",
            "publicUrl": "https://updated.example.com",
            "enabled": false,
        }]}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200, "顶层改名 + 设备白名单更新应成功");

    // delete
    let resp = reqwest::Client::new()
        .delete(format!("{base}/api/admin/users/{uid}"))
        .header("cookie", &cookie)
        .send()
        .await
        .unwrap();
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
        .send()
        .await
        .unwrap();
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

// ---------------------------------------------------------------------------
// Task 5：PUT 白名单收敛 + CreateUserBody 去设备
// ---------------------------------------------------------------------------

/// PUT 仅白名单字段生效：name/publicUrl/enabled 可改，
/// port/pctype/desc/deviceName/bound/deviceId 出现即静默忽略。
#[tokio::test]
async fn put_updates_only_whitelisted_fields() {
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

    let resp = reqwest::Client::new()
        .put(format!("{base}/api/admin/users/{}", user.id))
        .header("cookie", &cookie)
        .json(&serde_json::json!({"devices": [{
            "deviceId": "3f2b8a44-1c5d-4e6f-9a8b-7c2d1e0f9a8b",
            "name": "macbook-pro-v2",
            "publicUrl": "https://new.example.com",
            "enabled": false,
            // —— 以下全部为非白名单字段，必须被静默忽略 ——
            "port": 1234,
            "pctype": "windows",
            "desc": "恶意篡改",
            "deviceName": "Hacked-Box",
            "bound": true,
        }]}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);

    let users = admin_users(&base).await;
    let dev = &users["users"][0]["devices"][0];
    assert_eq!(dev["name"], "macbook-pro-v2", "服务名（白名单）应更新");
    assert_eq!(
        dev["publicUrl"], "https://new.example.com",
        "穿透地址（白名单）应更新"
    );
    assert_eq!(dev["enabled"], false, "enabled（白名单）应更新");
    assert_eq!(dev["port"], 9464, "port 非白名单，须被忽略");
    assert_eq!(dev["pctype"], "macos", "pctype 非白名单，须被忽略");
    assert_eq!(dev["desc"], "工作电脑", "desc 非白名单，须被忽略");
    assert_eq!(
        dev["deviceName"], "Samuels-MacBook",
        "deviceName 非白名单，须被忽略"
    );
    assert_eq!(dev["bound"], true, "bound 非白名单，须被忽略（存量值不变）");
    assert_eq!(
        dev["deviceId"], "3f2b8a44-1c5d-4e6f-9a8b-7c2d1e0f9a8b",
        "deviceId 是定位键，不得被改写"
    );
}

/// deviceId 定位失败 → 404「设备不存在」。
#[tokio::test]
async fn put_unknown_device_404() {
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

    let resp = reqwest::Client::new()
        .put(format!("{base}/api/admin/users/{}", user.id))
        .header("cookie", &cookie)
        .json(&serde_json::json!({"devices": [{
            "deviceId": "no-such-device",
            "name": "whatever",
            "publicUrl": "https://x.example.com",
        }]}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 404);
    let err: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(err["error"]["code"], "not_found");

    // 404 不得写回
    let users = admin_users(&base).await;
    assert_eq!(users["users"][0]["devices"][0]["name"], "macbook-pro");
}

/// deviceId 为空的存量设备回退按 name 定位；name 全局唯一校验排除自身。
#[tokio::test]
async fn put_duplicate_name_still_rejected() {
    let sb = MockServer::start().await;
    let mut user = test_user();
    user.devices[0].device_id = "3f2b8a44-1c5d-4e6f-9a8b-7c2d1e0f9a8b".into();
    let other = {
        let mut u = user.clone();
        u.id = "200002".into();
        u.name = "alice".into();
        u.key = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb".into();
        u.devices = vec![mini_oc_core::device::UserDevice {
            desc: "另一台".into(),
            name: "alice-box".into(),
            ..user.devices[0].clone()
        }];
        u
    };
    mount_stateful_sb(
        &sb,
        UsersFile {
            version: 2,
            users: vec![user, other],
        },
    )
    .await;
    let base = spawn_app(&sb).await;
    let cookie = admin_login(&base).await;

    // ① deviceId 为空 → 回退按 name "macbook-pro" 定位；同名幂等放行（排除自身）
    let resp = reqwest::Client::new()
        .put(format!("{base}/api/admin/users/100001"))
        .header("cookie", &cookie)
        .json(&serde_json::json!({"devices": [{
            "deviceId": "",
            "name": "macbook-pro",
            "publicUrl": "https://x.example.com",
        }]}))
        .send()
        .await
        .unwrap();
    assert_eq!(
        resp.status(),
        200,
        "deviceId 空 → 回退按 name 定位；同名幂等放行"
    );

    // ② deviceId 定位成功后改名撞其他用户设备名 → 400（全局唯一，排除自身后仍撞名）
    let resp = reqwest::Client::new()
        .put(format!("{base}/api/admin/users/100001"))
        .header("cookie", &cookie)
        .json(&serde_json::json!({"devices": [{
            "deviceId": "3f2b8a44-1c5d-4e6f-9a8b-7c2d1e0f9a8b",
            "name": "alice-box",
            "publicUrl": "https://x.example.com",
        }]}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 400, "撞其他用户设备名 → 400");
    let err: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(err["error"]["code"], "invalid_target");
}

/// Fix Round 2：deviceId 为空的存量设备改名——定位用原 name 经 `matchName` 携带。
///
/// 原缺陷：name 兼任「定位键」与「待改字段」，改名时按新名定位 → 404「设备不存在」。
#[tokio::test]
async fn put_rename_with_empty_device_id_via_match_name() {
    let sb = MockServer::start().await;
    let mut user = test_user();
    user.devices[0].device_id = String::new();
    user.devices[0].name = "old-name".into();
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

    let resp = reqwest::Client::new()
        .put(format!("{base}/api/admin/users/{}", user.id))
        .header("cookie", &cookie)
        .json(&serde_json::json!({"devices": [{
            "deviceId": "",
            "matchName": "old-name",
            "name": "new-name",
            "publicUrl": "https://t.example.com",
        }]}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200, "matchName 定位改名 → 200（原实现 404）");

    let users = admin_users(&base).await;
    let dev = &users["users"][0]["devices"][0];
    assert_eq!(dev["name"], "new-name", "落盘后服务名应为新值");
    assert_eq!(
        dev["publicUrl"], "https://t.example.com",
        "其余白名单字段一并更新"
    );
    assert_eq!(
        dev["matchName"],
        serde_json::Value::Null,
        "matchName 仅用于定位，不得写入设备"
    );
}

/// body 只填 name → 200，key 自动生成，devices 恒为空数组。
#[tokio::test]
async fn create_user_without_devices() {
    let sb = MockServer::start().await;
    mount_stateful_sb(
        &sb,
        UsersFile {
            version: 2,
            users: vec![],
        },
    )
    .await;
    let base = spawn_app(&sb).await;
    let cookie = admin_login(&base).await;

    let resp = reqwest::Client::new()
        .post(format!("{base}/api/admin/users"))
        .header("cookie", &cookie)
        .json(&serde_json::json!({"name": "bob"}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["user"]["name"], "bob");
    assert_eq!(
        body["user"]["key"].as_str().unwrap().len(),
        32,
        "key 自动生成"
    );
    assert_eq!(
        body["user"]["devices"].as_array().unwrap().len(),
        0,
        "devices 恒为空数组"
    );

    // 真实读路径复核：注册表里该用户 devices 为空
    let users = admin_users(&base).await;
    let u = users["users"]
        .as_array()
        .unwrap()
        .iter()
        .find(|u| u["name"] == "bob")
        .unwrap();
    assert_eq!(u["devices"].as_array().unwrap().len(), 0);
}
