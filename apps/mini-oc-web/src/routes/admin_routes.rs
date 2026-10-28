//! 管理端：超管登录 + 用户 CRUD（V2 设备字段集 + 安全修补）。
//!
//! 安全要点：
//! - 超管密码 = SB_PASSWORD，SHA-256 后常数时间比较；
//! - 管理 cookie（12h）与用户 cookie（7d）分离；
//! - 不存在 /api/admin/info（V1 sb_password 泄露端点已移除）。

use crate::auth::{sign_cookie, verify_cookie};
use crate::error::{AppError, AppResult};
use crate::routes::SharedState;
use axum::extract::{Path, State};
use axum::http::header::SET_COOKIE;
use axum::http::HeaderMap;
use axum::routing::{get, post};
use axum::{Json, Router};
use chrono::Utc;
use mini_oc_core::device::{generate_key, generate_user_id, PortalUser, UserDevice};
use sha2::{Digest, Sha256};

/// 管理会话 cookie 名（12 小时）。
pub const ADMIN_COOKIE: &str = "mini_oc_admin";
const ADMIN_TTL_SECS: u64 = 12 * 3600;

pub fn router() -> Router<SharedState> {
    Router::new()
        .route("/api/admin/login", post(admin_login))
        .route("/api/admin/logout", post(admin_logout))
        .route(
            "/api/admin/users",
            get(admin_users_list).post(admin_users_create),
        )
        .route(
            "/api/admin/users/:id",
            axum::routing::put(admin_users_update).delete(admin_users_delete),
        )
        .route(
            "/api/admin/users/:id/regenerate-key",
            post(admin_regenerate_key),
        )
        .route(
            "/api/admin/users/:id/devices/:did/approve",
            post(admin_device_approve),
        )
        .route(
            "/api/admin/users/:id/devices/:did/reject",
            axum::routing::delete(admin_device_reject),
        )
        .route(
            "/api/admin/users/:id/devices/:did",
            axum::routing::delete(admin_device_delete),
        )
}

/// 常数时间密码比较（SHA-256 摘要后逐字节异或累积，继承 V1）。
fn constant_time_eq(a: &str, b: &str) -> bool {
    let ha = Sha256::digest(a.as_bytes());
    let hb = Sha256::digest(b.as_bytes());
    ha.iter()
        .zip(hb.iter())
        .fold(0u8, |acc, (x, y)| acc | (x ^ y))
        == 0
}

/// 校验管理 cookie，返回 Ok(()) 或 401。
fn require_admin(state: &SharedState, headers: &HeaderMap) -> AppResult<()> {
    let raw = headers
        .get(axum::http::header::COOKIE)
        .and_then(|v| v.to_str().ok())
        .ok_or_else(|| AppError::Unauthorized("未登录".into()))?;
    let cookie = raw
        .split(';')
        .map(|s| s.trim())
        .find(|s| s.starts_with(ADMIN_COOKIE))
        .and_then(|s| s.strip_prefix(&format!("{ADMIN_COOKIE}=")))
        .ok_or_else(|| AppError::Unauthorized("非管理会话".into()))?;
    let subject = verify_cookie(cookie, &state.cookie_key)?;
    if subject != "admin" {
        return Err(AppError::Unauthorized("非管理会话".into()));
    }
    Ok(())
}

async fn admin_login(
    State(state): State<SharedState>,
    headers: HeaderMap,
    Json(body): Json<serde_json::Value>,
) -> AppResult<impl axum::response::IntoResponse> {
    // 独立 bucket 防爆破（I2：与用户登录同限流强度，SB_PASSWORD 是高价值目标）
    let ip = crate::routes::auth_routes::extract_ip(&headers);
    let bucket = format!("admin:{ip}");
    state.rate.check(&bucket).await?;
    let password = body["password"].as_str().unwrap_or_default();
    if password.is_empty() || !constant_time_eq(password, &state.admin_password) {
        state.rate.record_failure(&bucket).await;
        return Err(AppError::Unauthorized("密码错误".into()));
    }
    state.rate.reset(&bucket).await;
    let https = headers
        .get("x-forwarded-proto")
        .and_then(|v| v.to_str().ok())
        .map(|v| v.eq_ignore_ascii_case("https"))
        .unwrap_or(false);
    let value = sign_cookie(
        "admin",
        &state.cookie_key,
        std::time::Duration::from_secs(ADMIN_TTL_SECS),
    )?;
    let mut cookie =
        format!("{ADMIN_COOKIE}={value}; Path=/; HttpOnly; SameSite=Lax; Max-Age={ADMIN_TTL_SECS}");
    if https {
        cookie.push_str("; Secure");
    }
    let mut resp_headers = HeaderMap::new();
    resp_headers.insert(SET_COOKIE, cookie.parse().unwrap());
    Ok((resp_headers, Json(serde_json::json!({"ok": true}))))
}

async fn admin_logout() -> impl axum::response::IntoResponse {
    let mut resp_headers = HeaderMap::new();
    resp_headers.insert(
        SET_COOKIE,
        format!("{ADMIN_COOKIE}=; Path=/; HttpOnly; SameSite=Lax; Max-Age=0")
            .parse()
            .unwrap(),
    );
    (resp_headers, Json(serde_json::json!({"ok": true})))
}

async fn admin_users_list(
    State(state): State<SharedState>,
    headers: HeaderMap,
) -> AppResult<Json<serde_json::Value>> {
    require_admin(&state, &headers)?;
    let file = state.sb.get_users().await?;
    Ok(Json(serde_json::json!({"users": file.users})))
}

#[derive(serde::Deserialize)]
struct CreateUserBody {
    name: String,
}

/// 新增用户：只填用户名，密钥自动生成。
///
/// spec 决策 3：设备清单不再由管理员录入（`CreateUserBody.devices` 字段已删除，
/// 同仓库同步发版不做兼容）——设备改由客户端上报、管理员审核转正产生，
/// 新建用户 `devices` 恒为空数组。
async fn admin_users_create(
    State(state): State<SharedState>,
    headers: HeaderMap,
    Json(body): Json<CreateUserBody>,
) -> AppResult<impl axum::response::IntoResponse> {
    require_admin(&state, &headers)?;
    let mut file = state.sb.get_users().await?;

    if file.users.iter().any(|u| u.name == body.name) {
        return Err(AppError::InvalidTarget("用户名已存在".into()));
    }

    let now = Utc::now().to_rfc3339();
    let user = PortalUser {
        id: generate_user_id(&file.users.iter().map(|u| u.id.clone()).collect::<Vec<_>>()),
        name: body.name,
        key: generate_key(),
        devices: vec![],
        pending_devices: vec![],
        created_at: now.clone(),
        updated_at: now,
        last_used_at: None,
    };
    user.validate().map_err(AppError::InvalidTarget)?;
    file.users.push(user.clone());
    state.sb.put_users(&file).await?;
    Ok(Json(serde_json::json!({"user": user})))
}

/// PUT /api/admin/users/:id 请求体中的单条设备更新指令（白名单）。
///
/// 仅 `deviceId`（定位键）+ `name`/`publicUrl`/`enabled`（可改字段）参与反序列化；
/// `port`/`pctype`/`deviceName`/`bound`/`desc` 等事实字段出现即被 serde 静默忽略
/// （归客户端上报维护，管理员不可经 PUT 改写）。
#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct DeviceUpdateInput {
    /// 设备定位键；为空时回退按 `matchName` 定位，再无则回退按 `name` 定位
    /// （存量 device_id 空的设备兼容）
    #[serde(default)]
    device_id: String,
    /// 仅用于定位的原始服务名（deviceId 为空时的优先定位键；不写入设备）。
    /// 改名场景下 `name` 已是要改写成的新值，不能再兼任定位键。
    #[serde(default)]
    match_name: Option<String>,
    /// rathole 服务名（新值；deviceId 与 matchName 皆空时的回退定位键）
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    public_url: Option<String>,
    #[serde(default)]
    enabled: Option<bool>,
}

#[derive(serde::Deserialize)]
struct UpdateUserBody {
    /// 用户名（改名；spec 未删除的原有能力，唯一校验排除自身）
    #[serde(default)]
    name: Option<String>,
    /// 设备白名单更新指令；PUT 请求体不含 pendingDevices——待审核设备走 approve/reject 独立端点
    #[serde(default)]
    devices: Vec<DeviceUpdateInput>,
}

/// 定位设备：优先按 device_id 精确匹配；device_id 为空的存量条目优先按 matchName
/// （定位用原 name）匹配，无 matchName 时回退按 name 匹配（旧调用方语义不变）。
fn locate_device<'a>(devices: &'a [UserDevice], input: &'a DeviceUpdateInput) -> Option<usize> {
    if !input.device_id.is_empty() {
        return devices.iter().position(|d| d.device_id == input.device_id);
    }
    if let Some(match_name) = input
        .match_name
        .as_deref()
        .filter(|m| !m.is_empty())
        .or(input.name.as_deref())
    {
        return devices.iter().position(|d| d.name == match_name);
    }
    None
}

/// 编辑用户：用户名改名 + 设备逐条白名单更新。
///
/// spec「变更：PUT /api/admin/users/:uid」：
/// - 设备不再整体替换数组，改为按 `deviceId` 定位、仅改白名单字段
///   （`name`/`publicUrl`/`enabled`）；定位失败 404「设备不存在」。
/// - name 全局唯一校验保留（排除自身）。
/// - 所有写操作刷 updated_at。
async fn admin_users_update(
    State(state): State<SharedState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(body): Json<UpdateUserBody>,
) -> AppResult<Json<serde_json::Value>> {
    require_admin(&state, &headers)?;
    let mut file = state.sb.get_users().await?;
    let user_idx = file
        .users
        .iter()
        .position(|u| u.id == id)
        .ok_or_else(|| AppError::NotFound(format!("用户 {id} 不存在")))?;

    // —— 校验阶段（不可变借用）——
    // 用户名改名：全局唯一校验（排除自身）
    if let Some(name) = &body.name {
        if file.users.iter().any(|u| u.id != id && &u.name == name) {
            return Err(AppError::InvalidTarget("用户名已被占用".into()));
        }
    }
    // 设备逐条定位 + 服务名全局唯一（rathole service 名空间全局，排除自身其他设备）
    for input in &body.devices {
        let dev_idx = locate_device(&file.users[user_idx].devices, input)
            .ok_or_else(|| AppError::NotFound("设备不存在".into()))?;
        let orig = &file.users[user_idx].devices[dev_idx];
        let new_name = input.name.clone().unwrap_or_else(|| orig.name.clone());
        if new_name != orig.name
            && file.users.iter().any(|u| {
                u.devices
                    .iter()
                    .any(|x| x.name == new_name && !(u.id == id && x.name == orig.name))
            })
        {
            return Err(AppError::InvalidTarget(format!(
                "设备名 {} 已被使用",
                new_name
            )));
        }
    }

    // —— 写入阶段（可变借用）：仅白名单字段赋值 ——
    let slot = &mut file.users[user_idx];
    if let Some(name) = body.name {
        slot.name = name;
    }
    for input in &body.devices {
        let dev_idx = locate_device(&slot.devices, input)
            .ok_or_else(|| AppError::NotFound("设备不存在".into()))?;
        let dev = &mut slot.devices[dev_idx];
        if let Some(name) = &input.name {
            dev.name = name.clone();
        }
        if let Some(url) = &input.public_url {
            dev.public_url = url.clone();
        }
        if let Some(enabled) = input.enabled {
            dev.enabled = enabled;
        }
    }
    slot.updated_at = Utc::now().to_rfc3339();
    let updated = slot.clone();
    updated.validate().map_err(AppError::InvalidTarget)?;
    state.sb.put_users(&file).await?;
    Ok(Json(serde_json::json!({"user": updated})))
}

async fn admin_users_delete(
    State(state): State<SharedState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> AppResult<Json<serde_json::Value>> {
    require_admin(&state, &headers)?;
    let mut file = state.sb.get_users().await?;
    let before = file.users.len();
    file.users.retain(|u| u.id != id);
    if file.users.len() == before {
        return Err(AppError::NotFound(format!("用户 {id} 不存在")));
    }
    state.sb.put_users(&file).await?;
    Ok(Json(serde_json::json!({"ok": true})))
}

async fn admin_regenerate_key(
    State(state): State<SharedState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> AppResult<Json<serde_json::Value>> {
    require_admin(&state, &headers)?;
    let mut file = state.sb.get_users().await?;
    let slot = file
        .users
        .iter_mut()
        .find(|u| u.id == id)
        .ok_or_else(|| AppError::NotFound(format!("用户 {id} 不存在")))?;
    slot.key = generate_key();
    slot.updated_at = Utc::now().to_rfc3339();
    let key = slot.key.clone();
    state.sb.put_users(&file).await?;
    Ok(Json(serde_json::json!({"key": key})))
}

// ---------------------------------------------------------------------------
// 设备审核（approve / reject）与单设备移除（Task 4）
// ---------------------------------------------------------------------------

#[derive(serde::Deserialize)]
struct ApproveBody {
    /// rathole 服务名（全局唯一）
    name: String,
    /// 隧道公网地址
    #[serde(rename = "publicUrl")]
    public_url: String,
}

/// 设备待审核转正：pending → devices。
///
/// spec 约定：enabled:true、bound:false；desc 取上报值、空串以 device_name 兜底
/// （满足 UserDevice::validate 的 desc 非空要求）；port/pctype/device_id/device_name
/// 取上报值；name/publicUrl 取管理员输入。name 全局唯一校验范围 = 所有用户 devices
/// + 当前用户 pending_devices（pending 中同名待审设备也会在转正后撞名，提前拦截）。
async fn admin_device_approve(
    State(state): State<SharedState>,
    headers: HeaderMap,
    Path((id, did)): Path<(String, String)>,
    Json(body): Json<ApproveBody>,
) -> AppResult<Json<serde_json::Value>> {
    require_admin(&state, &headers)?;
    let mut file = state.sb.get_users().await?;
    let Some(pending) = file
        .users
        .iter()
        .find(|u| u.id == id)
        .and_then(|u| u.pending_devices.iter().find(|p| p.device_id == did))
        .cloned()
    else {
        return Err(AppError::NotFound(format!("待审核设备 {did} 不存在")));
    };

    // 构造转正设备（校验失败不触碰注册表）
    let device = UserDevice {
        desc: if pending.desc.is_empty() {
            pending.device_name.clone()
        } else {
            pending.desc.clone()
        },
        name: body.name,
        port: pending.port,
        pctype: pending.pctype.clone(),
        device_name: pending.device_name.clone(),
        bound: false,
        public_url: body.public_url,
        enabled: true,
        device_id: pending.device_id.clone(),
    };
    device.validate().map_err(AppError::InvalidTarget)?;
    // name 全局唯一（rathole service 名空间全局）：所有用户 devices + 当前用户 pending_devices
    let dup = file.users.iter().any(|u| {
        u.devices.iter().any(|x| x.name == device.name)
            || (u.id == id
                && u.pending_devices
                    .iter()
                    .any(|p| p.device_id != did && p_desc_name(p) == device.name))
    });
    if dup {
        return Err(AppError::InvalidTarget(format!(
            "设备名 {} 已被使用",
            device.name
        )));
    }

    // 写入：从 pending 移除、追加 devices、刷 updated_at
    let slot = file.users.iter_mut().find(|u| u.id == id).unwrap();
    slot.pending_devices.retain(|p| p.device_id != did);
    slot.devices.push(device);
    slot.updated_at = Utc::now().to_rfc3339();
    let updated = slot.clone();
    state.sb.put_users(&file).await?;
    Ok(Json(serde_json::json!({"user": updated})))
}

/// PendingDevice 无 name 字段——全局唯一校验对 pending 仅以 desc 近似对照。
///
/// 注：pending 阶段尚无 rathole 服务名，spec 校验范围要求「当前用户 pending_devices」
/// 实指防止对同一批待审设备审核时撞名。desc 是客户端可读标识，仅作近似对照：
/// 误报（desc 恰与新名相同而拒绝）可能性极低；漏报（desc 与将来定的 name 不同）
/// 由审核通过后写入 devices 时走全量唯一校验兜底。
fn p_desc_name(p: &mini_oc_core::device::PendingDevice) -> &str {
    &p.desc
}

/// 拒绝待审核设备：仅从 pending_devices 移除，无其他副作用。
async fn admin_device_reject(
    State(state): State<SharedState>,
    headers: HeaderMap,
    Path((id, did)): Path<(String, String)>,
) -> AppResult<Json<serde_json::Value>> {
    require_admin(&state, &headers)?;
    let mut file = state.sb.get_users().await?;
    let Some(slot) = file.users.iter_mut().find(|u| u.id == id) else {
        return Err(AppError::NotFound(format!("用户 {id} 不存在")));
    };
    let before = slot.pending_devices.len();
    slot.pending_devices.retain(|p| p.device_id != did);
    if slot.pending_devices.len() == before {
        return Err(AppError::NotFound(format!("待审核设备 {did} 不存在")));
    }
    slot.updated_at = Utc::now().to_rfc3339();
    state.sb.put_users(&file).await?;
    Ok(Json(serde_json::json!({"ok": true})))
}

/// 移除单台设备：仅允许删除停用（enabled == false）的设备，启用中 → 409。
async fn admin_device_delete(
    State(state): State<SharedState>,
    headers: HeaderMap,
    Path((id, did)): Path<(String, String)>,
) -> AppResult<Json<serde_json::Value>> {
    require_admin(&state, &headers)?;
    let mut file = state.sb.get_users().await?;
    let slot = file
        .users
        .iter_mut()
        .find(|u| u.id == id)
        .ok_or_else(|| AppError::NotFound(format!("用户 {id} 不存在")))?;
    let Some(dev) = slot.devices.iter().find(|d| d.device_id == did) else {
        return Err(AppError::NotFound(format!("设备 {did} 不存在")));
    };
    if dev.enabled {
        return Err(AppError::Conflict("启用中的设备不可删除，请先停用".into()));
    }
    let before = slot.devices.len();
    slot.devices.retain(|d| d.device_id != did);
    if slot.devices.len() == before {
        return Err(AppError::NotFound(format!("设备 {did} 不存在")));
    }
    slot.updated_at = Utc::now().to_rfc3339();
    state.sb.put_users(&file).await?;
    Ok(Json(serde_json::json!({"ok": true})))
}
