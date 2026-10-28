//! 登录 / 会话 / 自助接口：login、logout、me、me/key、me/name。

use crate::auth::{sign_cookie, verify_cookie};
use crate::error::{AppError, AppResult};
use crate::probe::probe_device;
use crate::routes::SharedState;
use axum::extract::{Request, State};
use axum::http::header::SET_COOKIE;
use axum::http::HeaderMap;
use axum::routing::{get, post};
use axum::{Json, Router};
use chrono::Utc;
use mini_oc_core::device::PortalUser;

/// 用户会话 cookie 名（7 天）。
pub const USER_COOKIE: &str = "mini_oc_session";
const USER_TTL_SECS: u64 = 7 * 24 * 3600;

pub fn router() -> Router<SharedState> {
    Router::new()
        .route("/api/login", post(login))
        .route("/api/logout", post(logout))
        .route("/api/me", get(me))
        .route("/api/me/key", get(me_key))
        .route("/api/me/name", post(me_name))
}

/// 提取客户端 IP（反代场景优先）。
pub fn extract_ip(headers: &HeaderMap) -> String {
    headers
        .get("x-real-ip")
        .and_then(|v| v.to_str().ok())
        .or_else(|| {
            headers
                .get("x-forwarded-for")
                .and_then(|v| v.to_str().ok())
                .and_then(|v| v.split(',').next())
        })
        .unwrap_or("unknown")
        .trim()
        .to_string()
}

/// cookie 会话解析 → 当前用户。
pub async fn current_user(state: &SharedState, headers: &HeaderMap) -> AppResult<PortalUser> {
    let raw = headers
        .get(axum::http::header::COOKIE)
        .and_then(|v| v.to_str().ok())
        .ok_or_else(|| AppError::Unauthorized("未登录".into()))?;
    let cookie = raw
        .split(';')
        .map(|s| s.trim())
        .find(|s| s.starts_with(USER_COOKIE))
        .and_then(|s| s.strip_prefix(&format!("{USER_COOKIE}=")))
        .ok_or_else(|| AppError::Unauthorized("未登录".into()))?;
    let subject = verify_cookie(cookie, &state.cookie_key)?;
    let id = subject
        .strip_prefix("u:")
        .ok_or_else(|| AppError::Unauthorized("会话主体非法".into()))?;
    let file = state.sb.get_users().await?;
    file.users
        .into_iter()
        .find(|u| u.id == id)
        .ok_or_else(|| AppError::Unauthorized("用户不存在或已删除".into()))
}

/// 是否按 https 下发（决定 Secure flag，修补 V1 债务）。
fn is_https(headers: &HeaderMap) -> bool {
    headers
        .get("x-forwarded-proto")
        .and_then(|v| v.to_str().ok())
        .map(|v| v.eq_ignore_ascii_case("https"))
        .unwrap_or(false)
}

fn build_session_cookie(subject: &str, key: &[u8], https: bool) -> AppResult<String> {
    let value = sign_cookie(subject, key, std::time::Duration::from_secs(USER_TTL_SECS))?;
    let mut cookie =
        format!("{USER_COOKIE}={value}; Path=/; HttpOnly; SameSite=Lax; Max-Age={USER_TTL_SECS}");
    if https {
        cookie.push_str("; Secure");
    }
    Ok(cookie)
}

async fn login(
    State(state): State<SharedState>,
    headers: HeaderMap,
    Json(body): Json<serde_json::Value>,
) -> AppResult<impl axum::response::IntoResponse> {
    let ip = extract_ip(&headers);
    state.rate.check(&ip).await.map_err(|e| {
        tracing::warn!(%ip, "登录被限流");
        e
    })?;
    let key = body["key"].as_str().unwrap_or_default();
    if key.is_empty() {
        return Err(AppError::Unauthorized("缺少密钥".into()));
    }
    let file = state.sb.get_users().await?;
    let Some(user) = file.users.iter().find(|u| u.key == key).cloned() else {
        state.rate.record_failure(&ip).await;
        return Err(AppError::Unauthorized("密钥错误".into()));
    };
    state.rate.reset(&ip).await;

    // best-effort 更新 last_used_at（失败不阻塞登录）
    let mut updated = user.clone();
    updated.last_used_at = Some(Utc::now().to_rfc3339());
    let mut users = file.users;
    if let Some(slot) = users.iter_mut().find(|u| u.id == user.id) {
        *slot = updated;
    }
    if let Err(e) = state
        .sb
        .put_users(&mini_oc_core::device::UsersFile { version: 2, users })
        .await
    {
        tracing::warn!(%e, "last_used_at 写回失败（不阻塞登录）");
    }

    let https = is_https(&headers);
    let cookie = build_session_cookie(&format!("u:{}", user.id), &state.cookie_key, https)?;
    let mut resp_headers = HeaderMap::new();
    resp_headers.insert(SET_COOKIE, cookie.parse().unwrap());
    Ok((
        resp_headers,
        Json(serde_json::json!({"id": user.id, "name": user.name})),
    ))
}

async fn logout() -> impl axum::response::IntoResponse {
    let mut resp_headers = HeaderMap::new();
    resp_headers.insert(
        SET_COOKIE,
        format!("{USER_COOKIE}=; Path=/; HttpOnly; SameSite=Lax; Max-Age=0")
            .parse()
            .unwrap(),
    );
    (resp_headers, Json(serde_json::json!({"ok": true})))
}

async fn me(
    State(state): State<SharedState>,
    headers: HeaderMap,
) -> AppResult<impl axum::response::IntoResponse> {
    let user = current_user(&state, &headers).await?;
    // 停用设备不下发也不探测（先 filter 再 join_all）；admin 列表不过滤，管理员可见
    let enabled: Vec<&mini_oc_core::device::UserDevice> =
        user.devices.iter().filter(|d| d.enabled).collect();
    // 并发探测全部启用设备（永不抛错，慢设备 4s 超时兜底）
    let probes = futures_probe_all(&user, &enabled).await;
    let devices: Vec<serde_json::Value> = enabled
        .iter()
        .zip(probes)
        .map(|(d, p)| {
            let mut v = serde_json::to_value(d).unwrap_or_default();
            v["status"] = serde_json::json!(p.status_str());
            v["version"] = p
                .version()
                .map(|s| serde_json::json!(s))
                .unwrap_or(serde_json::Value::Null);
            v
        })
        .collect();
    Ok(Json(serde_json::json!({
        "user": {"id": user.id, "name": user.name},
        "devices": devices,
    })))
}

async fn futures_probe_all(
    user: &PortalUser,
    devices: &[&mini_oc_core::device::UserDevice],
) -> Vec<crate::probe::DeviceProbe> {
    futures::future::join_all(devices.iter().map(|d| probe_device(d, &user.key))).await
}

async fn me_key(
    State(state): State<SharedState>,
    headers: HeaderMap,
) -> AppResult<impl axum::response::IntoResponse> {
    let user = current_user(&state, &headers).await?;
    Ok(Json(serde_json::json!({"key": user.key})))
}

#[derive(serde::Deserialize)]
struct NameBody {
    name: String,
}

async fn me_name(
    State(state): State<SharedState>,
    headers: HeaderMap,
    Json(body): Json<NameBody>,
) -> AppResult<impl axum::response::IntoResponse> {
    let mut user = current_user(&state, &headers).await?;
    let trimmed = body.name.trim().to_string();
    user.name = trimmed;
    user.validate().map_err(|e| AppError::InvalidTarget(e))?;
    user.updated_at = Utc::now().to_rfc3339();

    let mut file = state.sb.get_users().await?;
    // 重名检查（其他用户）
    if file
        .users
        .iter()
        .any(|u| u.id != user.id && u.name == user.name)
    {
        return Err(AppError::InvalidTarget("名称已被占用".into()));
    }
    if let Some(slot) = file.users.iter_mut().find(|u| u.id == user.id) {
        *slot = user.clone();
    }
    state.sb.put_users(&file).await?;
    Ok(Json(serde_json::json!({"ok": true, "name": user.name})))
}

/// 供其他路由模块复用的请求封装（消除重复）。
pub async fn require_user(req: &Request, state: &SharedState) -> AppResult<PortalUser> {
    current_user(state, req.headers()).await
}
