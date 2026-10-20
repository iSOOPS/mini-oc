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
        .route("/api/admin/users", get(admin_users_list).post(admin_users_create))
        .route("/api/admin/users/:id", axum::routing::put(admin_users_update).delete(admin_users_delete))
        .route("/api/admin/users/:id/regenerate-key", post(admin_regenerate_key))
}

/// 常数时间密码比较（SHA-256 摘要后逐字节异或累积，继承 V1）。
fn constant_time_eq(a: &str, b: &str) -> bool {
    let ha = Sha256::digest(a.as_bytes());
    let hb = Sha256::digest(b.as_bytes());
    ha.iter().zip(hb.iter()).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
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
    let password = body["password"].as_str().unwrap_or_default();
    if password.is_empty() || !constant_time_eq(password, &state.admin_password) {
        return Err(AppError::Unauthorized("密码错误".into()));
    }
    let https = headers
        .get("x-forwarded-proto")
        .and_then(|v| v.to_str().ok())
        .map(|v| v.eq_ignore_ascii_case("https"))
        .unwrap_or(false);
    let value = sign_cookie("admin", &state.cookie_key, std::time::Duration::from_secs(ADMIN_TTL_SECS))?;
    let mut cookie = format!("{ADMIN_COOKIE}={value}; Path=/; HttpOnly; SameSite=Lax; Max-Age={ADMIN_TTL_SECS}");
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
            return Err(AppError::InvalidTarget(format!("设备名 {} 已被使用", dev.name)));
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
                    return Err(AppError::InvalidTarget(format!("设备名 {} 已被使用", dev.name)));
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
