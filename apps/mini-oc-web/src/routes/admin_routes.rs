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
    #[serde(default)]
    devices: Vec<serde_json::Value>,
}

async fn admin_users_create(
    State(state): State<SharedState>,
    headers: HeaderMap,
    Json(body): Json<CreateUserBody>,
) -> AppResult<impl axum::response::IntoResponse> {
    require_admin(&state, &headers)?;
    let mut file = state.sb.get_users().await?;

    // 设备解析 + 校验
    let mut devices = Vec::new();
    for d in &body.devices {
        let dev: UserDevice = serde_json::from_value(d.clone())
            .map_err(|e| AppError::InvalidTarget(format!("设备字段错误: {e}")))?;
        dev.validate().map_err(|e| AppError::InvalidTarget(e))?;
        // 全局唯一（rathole service 名空间全局）
        let dup = file
            .users
            .iter()
            .any(|u| u.devices.iter().any(|x| x.name == dev.name));
        if dup {
            return Err(AppError::InvalidTarget(format!(
                "设备名 {} 已被使用",
                dev.name
            )));
        }
        devices.push(dev);
    }
    if file.users.iter().any(|u| u.name == body.name) {
        return Err(AppError::InvalidTarget("用户名已存在".into()));
    }

    let now = Utc::now().to_rfc3339();
    let user = PortalUser {
        id: generate_user_id(&file.users.iter().map(|u| u.id.clone()).collect::<Vec<_>>()),
        name: body.name,
        key: generate_key(),
        devices,
        pending_devices: vec![],
        created_at: now.clone(),
        updated_at: now,
        last_used_at: None,
    };
    user.validate().map_err(|e| AppError::InvalidTarget(e))?;
    file.users.push(user.clone());
    state.sb.put_users(&file).await?;
    Ok(Json(serde_json::json!({"user": user})))
}

async fn admin_users_update(
    State(state): State<SharedState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(body): Json<serde_json::Value>,
) -> AppResult<Json<serde_json::Value>> {
    require_admin(&state, &headers)?;
    let mut file = state.sb.get_users().await?;
    if !file.users.iter().any(|u| u.id == id) {
        return Err(AppError::NotFound(format!("用户 {id} 不存在")));
    }

    // —— 校验阶段（不可变借用）——
    let new_name = body["name"].as_str().map(|s| s.to_string());
    if let Some(name) = &new_name {
        if file.users.iter().any(|u| u.id != id && &u.name == name) {
            return Err(AppError::InvalidTarget("用户名已被占用".into()));
        }
    }
    let new_devices: Option<Vec<UserDevice>> = match body["devices"].as_array() {
        Some(devs) => {
            let mut parsed = Vec::new();
            for d in devs {
                let dev: UserDevice = serde_json::from_value(d.clone())
                    .map_err(|e| AppError::InvalidTarget(format!("设备字段错误: {e}")))?;
                dev.validate().map_err(|e| AppError::InvalidTarget(e))?;
                let dup_other = file
                    .users
                    .iter()
                    .filter(|u| u.id != id)
                    .any(|u| u.devices.iter().any(|x| x.name == dev.name));
                let dup_self = parsed.iter().any(|x: &UserDevice| x.name == dev.name);
                if dup_other || dup_self {
                    return Err(AppError::InvalidTarget(format!(
                        "设备名 {} 已被使用",
                        dev.name
                    )));
                }
                parsed.push(dev);
            }
            Some(parsed)
        }
        None => None,
    };

    // —— 写入阶段（可变借用）——
    let slot = file.users.iter_mut().find(|u| u.id == id).unwrap();
    if let Some(name) = new_name {
        slot.name = name;
    }
    if let Some(devices) = new_devices {
        slot.devices = devices;
    }
    slot.updated_at = Utc::now().to_rfc3339();
    slot.validate().map_err(|e| AppError::InvalidTarget(e))?;
    let updated = slot.clone();
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

/// PendingDevice 无 name 字段——全局唯一校验对 pending 取 desc 与 device_name 对照。
///
/// 注：pending 阶段尚无 rathole 服务名，spec 校验范围要求「当前用户 pending_devices」
/// 实指防止对同一批待审设备审核时撞名；此处以 desc（客户端可读标识）近似对照。
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
