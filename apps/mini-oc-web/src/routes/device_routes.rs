//! 设备信息下发 / 绑定 / 深探测（Task 7）。
//! 项目/会话列表与跳转在 Task 8/9 填充。

use crate::error::{AppError, AppResult};
use crate::probe::probe_device;
use crate::routes::auth_routes::{current_user, extract_ip};
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
        .route("/api/device-register", post(device_register))
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
    // 停用设备不下发（视为不存在；admin 列表不过滤，管理员可见）
    let devices: Vec<&mini_oc_core::device::UserDevice> =
        user.devices.iter().filter(|d| d.enabled).collect();
    Ok(Json(serde_json::json!({
        "id": user.id,
        "name": user.name,
        "key": user.key,
        "devices": devices,
    })))
}

#[derive(serde::Deserialize)]
#[allow(dead_code)]
struct BindBody {
    key: String,
    /// 客户端统一 camelCase 契约（I 修复：原 snake_case user_id 导致
    /// 客户端 device-bind 全部 422 Unprocessable Entity）
    #[serde(rename = "userId")]
    user_id: String,
    name: String,
    bound: bool,
    /// 登记的平台类型（与 pctype 语义一致，保留用于日志与未来扩展）
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
    let Some(user_id) = file
        .users
        .iter()
        .find(|u| u.key == body.key)
        .map(|u| u.id.clone())
    else {
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
        .filter(|d| d.enabled)
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
    let dev_slot = user_slot
        .devices
        .iter_mut()
        .find(|d| d.name == body.name)
        .unwrap();
    dev_slot.bound = body.bound;
    dev_slot.device_name = body.device_name;
    state.sb.put_users(&file).await?;
    Ok(Json(serde_json::json!({"ok": true})))
}

#[derive(serde::Deserialize)]
struct RegisterBody {
    key: String,
    /// 客户端持久化的设备 ID；首次上报为空
    #[serde(rename = "deviceId", default)]
    device_id: String,
    /// 客户端上报的描述（desc 归客户端，可空串）
    #[serde(default)]
    desc: String,
    port: u16,
    pctype: String,
    #[serde(rename = "deviceName")]
    device_name: String,
    /// 客户端真实平台（与 pctype 一致性校验）
    #[serde(default)]
    os: String,
}

/// 设备上报注册：按 spec 四态匹配做幂等处理。
///
/// 匹配顺序（spec 定夺版，否则存量设备会困在 pending）：
/// ① deviceId 非空 → 按 device_id 匹配 devices（命中 → 幂等更新事实字段，enabled 不变）
/// ② deviceId 空/未命中 → 按 device_name+pctype 匹配 devices（存量回填，approved）
/// ③ 按 device_name+pctype 匹配 pending_devices（更新 desc/port/reported_at，复用 deviceId）
/// ④ 生成新 PendingDevice 入 pending_devices（pending 态 enabled 恒 false）
async fn device_register(
    State(state): State<SharedState>,
    headers: HeaderMap,
    Json(body): Json<RegisterBody>,
) -> AppResult<impl axum::response::IntoResponse> {
    use mini_oc_core::device::{generate_device_id, PendingDevice};

    let ip = extract_ip(&headers);
    state.rate.check(&ip).await?;
    let mut file = state.sb.get_users().await?;
    let Some(user_slot) = file.users.iter_mut().find(|u| u.key == body.key) else {
        state.rate.record_failure(&ip).await;
        return Err(AppError::Unauthorized("密钥错误".into()));
    };
    state.rate.reset(&ip).await;

    // 平台一致性校验（同 device-bind：客户端上报 os 必须与上报 pctype 一致）
    if !body.os.is_empty() && !body.pctype.is_empty() && body.os != body.pctype {
        return Err(AppError::InvalidTarget(format!(
            "平台不匹配：pctype 为 {}，客户端上报 os 为 {}",
            body.pctype, body.os
        )));
    }
    let pending = PendingDevice {
        device_id: String::new(), // 由下方匹配流程填充
        desc: body.desc.clone(),
        port: body.port,
        pctype: body.pctype.clone(),
        device_name: body.device_name.clone(),
        reported_at: Utc::now().to_rfc3339(),
    };
    pending.validate().map_err(AppError::InvalidTarget)?;

    // —— ① deviceId 非空：按 device_id 匹配 devices，命中 → 幂等更新事实字段 ——
    if !body.device_id.is_empty() {
        if let Some(dev) = user_slot
            .devices
            .iter_mut()
            .find(|d| d.device_id == body.device_id)
        {
            dev.port = body.port;
            dev.pctype = body.pctype.clone();
            dev.device_name = body.device_name.clone();
            // UserDevice::validate 要求 desc 非空：空串表示「本次未上报」，保留原值
            // （存量设备 desc 已过校验必然非空），否则 admin PUT 编辑该用户会被 400 锁死
            if !body.desc.is_empty() {
                dev.desc = body.desc.clone();
            }
            user_slot.updated_at = Utc::now().to_rfc3339();
            let enabled = dev.enabled;
            state.sb.put_users(&file).await?;
            return Ok(Json(serde_json::json!({
                "status": "approved",
                "deviceId": body.device_id,
                "enabled": enabled,
            })));
        }
    }

    // —— ② 存量回填：device_name+pctype 命中 devices → 回填 device_id，approved ——
    if let Some(dev) = user_slot
        .devices
        .iter_mut()
        .find(|d| d.device_name == body.device_name && d.pctype == body.pctype)
    {
        dev.device_id = if body.device_id.is_empty() {
            generate_device_id()
        } else {
            body.device_id.clone()
        };
        dev.port = body.port;
        dev.pctype = body.pctype.clone();
        dev.device_name = body.device_name.clone();
        // 同分支①：空 desc 保留原值，防 admin PUT 被 UserDevice::validate 400 锁死
        if !body.desc.is_empty() {
            dev.desc = body.desc.clone();
        }
        user_slot.updated_at = Utc::now().to_rfc3339();
        let (device_id, enabled) = (dev.device_id.clone(), dev.enabled);
        state.sb.put_users(&file).await?;
        return Ok(Json(serde_json::json!({
            "status": "approved",
            "deviceId": device_id,
            "enabled": enabled,
        })));
    }

    // —— ③ pending 匹配：device_name+pctype 命中 pending_devices → 更新记录，复用 deviceId ——
    if let Some(p) = user_slot
        .pending_devices
        .iter_mut()
        .find(|p| p.device_name == body.device_name && p.pctype == body.pctype)
    {
        p.desc = body.desc.clone();
        p.port = body.port;
        p.reported_at = Utc::now().to_rfc3339();
        user_slot.updated_at = Utc::now().to_rfc3339();
        let device_id = p.device_id.clone();
        state.sb.put_users(&file).await?;
        return Ok(Json(serde_json::json!({
            "status": "pending",
            "deviceId": device_id,
            "enabled": false,
        })));
    }

    // —— ④ 全新设备：生成 deviceId 追加 pending ——
    let device_id = generate_device_id();
    user_slot.pending_devices.push(PendingDevice {
        device_id: device_id.clone(),
        ..pending
    });
    user_slot.updated_at = Utc::now().to_rfc3339();
    state.sb.put_users(&file).await?;
    Ok(Json(serde_json::json!({
        "status": "pending",
        "deviceId": device_id,
        "enabled": false,
    })))
}

/// 单设备深探测（按名，cookie 会话）。
async fn device_status(
    State(state): State<SharedState>,
    headers: HeaderMap,
    Path(name): Path<String>,
) -> AppResult<impl axum::response::IntoResponse> {
    let user = current_user(&state, &headers).await?;
    let Some(device) = user
        .devices
        .iter()
        .find(|d| d.name == name)
        .filter(|d| d.enabled)
    else {
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
) -> AppResult<(
    mini_oc_core::device::PortalUser,
    mini_oc_core::device::UserDevice,
)> {
    let user = current_user(state, headers).await?;
    let Some(device) = user
        .devices
        .iter()
        .find(|d| d.name == name)
        .filter(|d| d.enabled)
        .cloned()
    else {
        return Err(AppError::NotFound(format!("设备 {name} 不存在")));
    };
    Ok((user, device))
}

/// 构造设备的 OcClient（basic opencode:<user.key>）。
fn device_client(
    user: &mini_oc_core::device::PortalUser,
    device: &mini_oc_core::device::UserDevice,
) -> AppResult<OcClient> {
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
    let cache_key = format!(
        "{}:{}:sessions:{}",
        user.id,
        name,
        directory.clone().unwrap_or_default()
    );
    if let Some(cached) = state.live_cache.get(&cache_key).await {
        return Ok(Json(cached));
    }
    let live = match device_client(&user, &device) {
        Ok(client) => client.sessions(directory.as_deref()).await.map_err(oc_err),
        Err(e) => Err(e),
    };
    let mut resp = dual_source(state.sb.clone(), &user.id, &name, "sessions", live).await?;
    // I8 修复：快照按 device 共享一份——回退数据必须按请求 directory 过滤
    if resp["source"] == "snapshot" {
        if let Some(dir) = &directory {
            if let Some(list) = resp["data"].as_array_mut() {
                list.retain(|s| s["directory"].as_str() == Some(dir.as_str()));
            }
        }
    }
    if resp["source"] == "live" {
        state.live_cache.put(&cache_key, resp.clone()).await;
    }
    Ok(Json(resp))
}

// ---------------------------------------------------------------------------
// 会话跳转 + 设备详情（Task 9）
// ---------------------------------------------------------------------------

/// 跳转：BFF 代设备签发 pair code，有 session 时换 token 拼会话深链，
/// 否则返回一次性 connect 链接。设备不可达/凭据不一致 → 显式 502。
async fn jump(
    State(state): State<SharedState>,
    headers: HeaderMap,
    Path(name): Path<String>,
    Query(params): Query<HashMap<String, String>>,
) -> AppResult<axum::response::Response> {
    use axum::http::StatusCode;

    let (user, device) = find_device(&state, &headers, &name).await?;
    let session = params.get("session").cloned();

    let tunnel = tunnel_base_url(&device.public_url, device.port)
        .map_err(|e| AppError::InvalidTarget(e.to_string()))?;
    let client = OcClient::new(&tunnel, &user.key);
    let code = client.pair().await.map_err(oc_err)?;

    // connect 链接（旧行为：登录到 Web UI 首页，无会话定位）
    let connect_url = || -> AppResult<String> {
        let u = crate::jump::connect_url(&tunnel, &code);
        crate::jump::validate_jump_url(&u, &tunnel)?;
        Ok(u)
    };

    // 有 session 时：code 换 token → 深链定位会话；换 token 失败回退 connect 链接
    let url = match session.as_deref().filter(|s| !s.is_empty()) {
        Some(sid) => match client.connect(&code).await {
            Ok(token) => crate::jump::session_deep_link(&tunnel, sid, &token)?,
            Err(e) => {
                tracing::warn!(%e, "pair code 换 token 失败，回退 connect 链接");
                connect_url()?
            }
        },
        None => connect_url()?,
    };

    if params.get("format").map(|f| f == "json").unwrap_or(false) {
        let body = serde_json::json!({"url": url, "expiresIn": 300});
        Ok((StatusCode::OK, Json(body)).into_response())
    } else {
        Ok((StatusCode::FOUND, [(axum::http::header::LOCATION, url)]).into_response())
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
