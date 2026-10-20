//! 设备信息下发 / 绑定 / 深探测（Task 7）。
//! 项目/会话列表与跳转在 Task 8/9 填充。

use crate::routes::auth_routes::{current_user, extract_ip};
use crate::error::{AppError, AppResult};
use crate::probe::probe_device;
use crate::routes::SharedState;
use crate::snapshot::dual_source;
use axum::extract::{Path, Query, State};
use axum::http::HeaderMap;
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::{Json, Router};
use chrono::Utc;
use mini_oc_core::ocapi::{OcApi, OcClient, OcError};
use mini_oc_core::tunnel::tunnel_base_url;
use std::collections::HashMap;

pub fn router() -> Router<SharedState> {
    Router::new()
        .route("/api/user/info", post(user_info))
        .route("/api/device-bind", post(device_bind))
        .route("/api/device-status/:name", get(device_status))
        .route("/api/devices/:name/projects", get(projects))
        .route("/api/devices/:name/sessions", get(sessions))
        .route("/api/devices/:name/jump", get(jump))
        .route("/api/devices/:name/detail", get(detail))
}

/// 设备端拉取属主完整配置（凭 key）。
///
/// V2：不再包含用户级 SB 配置块；devices 为 V2 字段集。
async fn user_info(
    State(state): State<SharedState>,
    headers: HeaderMap,
    Json(body): Json<serde_json::Value>,
) -> AppResult<impl axum::response::IntoResponse> {
    let ip = extract_ip(&headers);
    state.rate.check(&ip).await?;
    let key = body["key"].as_str().unwrap_or_default();
    let file = state.sb.get_users().await?;
    let Some(user) = file.users.iter().find(|u| u.key == key) else {
        state.rate.record_failure(&ip).await;
        return Err(AppError::Unauthorized("密钥错误".into()));
    };
    state.rate.reset(&ip).await;
    Ok(Json(serde_json::json!({
        "id": user.id,
        "name": user.name,
        "key": user.key,
        "devices": user.devices,
    })))
}

#[derive(serde::Deserialize)]
struct BindBody {
    key: String,
    user_id: String,
    name: String,
    bound: bool,
    #[serde(default)]
    pctype: String,
    #[serde(rename = "deviceName")]
    device_name: String,
    /// 客户端上报的真实平台（身份核验，防共址误染）
    #[serde(default)]
    os: String,
}

/// 设备自注册绑定：写 bound/device_name，校验 key 属主与平台一致。
async fn device_bind(
    State(state): State<SharedState>,
    headers: HeaderMap,
    Json(body): Json<BindBody>,
) -> AppResult<impl axum::response::IntoResponse> {
    let ip = extract_ip(&headers);
    state.rate.check(&ip).await?;
    let mut file = state.sb.get_users().await?;
    let Some(user_id) = file.users.iter().find(|u| u.key == body.key).map(|u| u.id.clone()) else {
        state.rate.record_failure(&ip).await;
        return Err(AppError::Unauthorized("密钥错误".into()));
    };
    state.rate.reset(&ip).await;
    if user_id != body.user_id {
        return Err(AppError::Forbidden("密钥不属于该用户".into()));
    }
    let Some(device_pctype) = file
        .users
        .iter()
        .find(|u| u.id == user_id)
        .and_then(|u| u.devices.iter().find(|d| d.name == body.name))
        .map(|d| d.pctype.clone())
    else {
        return Err(AppError::NotFound(format!("设备 {} 不存在", body.name)));
    };
    // 平台核验：客户端上报 os 必须与登记 pctype 一致（V1 踩坑后的防御）
    if !body.os.is_empty() && !device_pctype.is_empty() && body.os != device_pctype {
        return Err(AppError::Forbidden(format!(
            "平台不匹配：设备登记为 {device_pctype}，客户端上报为 {}",
            body.os
        )));
    }
    // 写回（通过 file 重新定位避免借用冲突）
    let now = Utc::now().to_rfc3339();
    let user_slot = file.users.iter_mut().find(|u| u.id == user_id).unwrap();
    user_slot.updated_at = now.clone();
    let dev_slot = user_slot.devices.iter_mut().find(|d| d.name == body.name).unwrap();
    dev_slot.bound = body.bound;
    dev_slot.device_name = body.device_name;
    state.sb.put_users(&file).await?;
    Ok(Json(serde_json::json!({"ok": true})))
}

/// 单设备深探测（按名，cookie 会话）。
async fn device_status(
    State(state): State<SharedState>,
    headers: HeaderMap,
    Path(name): Path<String>,
) -> AppResult<impl axum::response::IntoResponse> {
    let user = current_user(&state, &headers).await?;
    let Some(device) = user.devices.iter().find(|d| d.name == name) else {
        return Err(AppError::NotFound(format!("设备 {name} 不存在")));
    };
    let probe = probe_device(device, &user.key).await;
    Ok(Json(serde_json::json!({
        "name": name,
        "status": probe.status_str(),
        "version": probe.version(),
    })))
}

// ---------------------------------------------------------------------------
// 项目 / 会话双源列表（Task 8）
// ---------------------------------------------------------------------------

/// 定位当前用户的指定设备。
async fn find_device(
    state: &SharedState,
    headers: &HeaderMap,
    name: &str,
) -> AppResult<(mini_oc_core::device::PortalUser, mini_oc_core::device::UserDevice)> {
    let user = current_user(state, headers).await?;
    let Some(device) = user.devices.iter().find(|d| d.name == name).cloned() else {
        return Err(AppError::NotFound(format!("设备 {name} 不存在")));
    };
    Ok((user, device))
}

/// 构造设备的 OcClient（basic opencode:<user.key>）。
fn device_client(user: &mini_oc_core::device::PortalUser, device: &mini_oc_core::device::UserDevice) -> AppResult<OcClient> {
    let base = tunnel_base_url(&device.public_url, device.port)
        .map_err(|e| AppError::InvalidTarget(e.to_string()))?;
    Ok(OcClient::new(&base, &user.key))
}

/// OcError → AppError 映射（保持双源判别语义）。
fn oc_err(e: OcError) -> AppError {
    match e {
        OcError::Unauthorized(code) => {
            AppError::AuthMismatch(format!("设备凭据不一致 (HTTP {code})"))
        }
        OcError::Transport => AppError::DeviceOffline("设备不可达".into()),
        OcError::Other(m) => AppError::DeviceOffline(m),
    }
}

async fn projects(
    State(state): State<SharedState>,
    headers: HeaderMap,
    Path(name): Path<String>,
) -> AppResult<Json<serde_json::Value>> {
    let (user, device) = find_device(&state, &headers, &name).await?;
    let cache_key = format!("{}:{}:projects", user.id, name);
    if let Some(cached) = state.live_cache.get(&cache_key).await {
        return Ok(Json(cached));
    }
    let live = match device_client(&user, &device) {
        Ok(client) => client.projects().await.map_err(oc_err),
        Err(e) => Err(e),
    };
    let resp = dual_source(state.sb.clone(), &user.id, &name, "projects", live).await?;
    // 仅 live 结果进入缓存
    if resp["source"] == "live" {
        state.live_cache.put(&cache_key, resp.clone()).await;
    }
    Ok(Json(resp))
}

async fn sessions(
    State(state): State<SharedState>,
    headers: HeaderMap,
    Path(name): Path<String>,
    Query(params): Query<HashMap<String, String>>,
) -> AppResult<Json<serde_json::Value>> {
    let (user, device) = find_device(&state, &headers, &name).await?;
    let directory = params.get("directory").cloned();
    let cache_key = format!("{}:{}:sessions:{}", user.id, name, directory.clone().unwrap_or_default());
    if let Some(cached) = state.live_cache.get(&cache_key).await {
        return Ok(Json(cached));
    }
    let live = match device_client(&user, &device) {
        Ok(client) => client.sessions(directory.as_deref()).await.map_err(oc_err),
        Err(e) => Err(e),
    };
    let resp = dual_source(state.sb.clone(), &user.id, &name, "sessions", live).await?;
    if resp["source"] == "live" {
        state.live_cache.put(&cache_key, resp.clone()).await;
    }
    Ok(Json(resp))
}

// ---------------------------------------------------------------------------
// 会话跳转 + 设备详情（Task 9）
// ---------------------------------------------------------------------------

/// 跳转：BFF 代设备签发 pair code → 返回/302 一次性 connect 链接。
/// 设备不可达/凭据不一致 → 显式 502，无任何 legacy 回退。
async fn jump(
    State(state): State<SharedState>,
    headers: HeaderMap,
    Path(name): Path<String>,
    Query(params): Query<HashMap<String, String>>,
) -> AppResult<axum::response::Response> {
    use axum::http::StatusCode;
    use axum::response::Redirect;

    let (user, device) = find_device(&state, &headers, &name).await?;
    let _directory = params.get("directory"); // 目录参数由设备端 connect 会话上下文使用
    let _session = params.get("session");

    let tunnel = tunnel_base_url(&device.public_url, device.port)
        .map_err(|e| AppError::InvalidTarget(e.to_string()))?;
    let client = OcClient::new(&tunnel, &user.key);
    let code = client.pair().await.map_err(oc_err)?;
    let url = crate::jump::connect_url(&tunnel, &code);
    crate::jump::validate_jump_url(&url, &tunnel)?;

    if params.get("format").map(|f| f == "json").unwrap_or(false) {
        let body = serde_json::json!({"url": url, "expiresIn": 300});
        Ok((StatusCode::OK, Json(body)).into_response())
    } else {
        Ok((
            StatusCode::FOUND,
            [(axum::http::header::LOCATION, url)],
        )
            .into_response())
    }
}

/// 设备连接详情：仅隧道地址与端口信息，**不含任何凭据**（V1 债务修补）。
async fn detail(
    State(state): State<SharedState>,
    headers: HeaderMap,
    Path(name): Path<String>,
) -> AppResult<Json<serde_json::Value>> {
    let (_user, device) = find_device(&state, &headers, &name).await?;
    let tunnel_base = tunnel_base_url(&device.public_url, device.port)
        .map_err(|e| AppError::InvalidTarget(e.to_string()))?;
    Ok(Json(serde_json::json!({
        "name": device.name,
        "tunnelBase": tunnel_base,
        "port": device.port,
        "pctype": device.pctype,
        "bound": device.bound,
    })))
}
