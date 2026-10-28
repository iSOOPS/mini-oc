//! 账户模块：登录（/api/user/info）、设备绑定（/api/device-bind）、登出。
//!
//! 出站 HTTP 统一脱敏日志：key 永不落日志明文。

use crate::config::{load_config, save_config, ClientConfig};
use serde::{Deserialize, Serialize};

/// 设备摘要（下发清单的字段子集，客户端用）。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DeviceBrief {
    pub name: String,
    pub port: u16,
    pub pctype: String,
    pub bound: bool,
    #[serde(rename = "deviceName")]
    pub device_name: String,
    /// 隧道公网地址（rathole remote_addr 推导源）
    #[serde(default)]
    pub public_url: String,
}

/// 登录后返回的账户信息。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccountInfo {
    pub id: String,
    pub name: String,
    pub devices: Vec<DeviceBrief>,
}

fn http() -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .expect("reqwest client")
}

/// key 脱敏（日志安全）。
fn mask(key: &str) -> String {
    if key.len() <= 8 {
        "***".into()
    } else {
        format!("{}…{}", &key[..4], &key[key.len() - 4..])
    }
}

/// 登录：拉取属主配置并持久化（失败不污染现有配置）。
pub async fn login(server_url: &str, key: &str) -> Result<AccountInfo, String> {
    tracing::info!("登录 {}（密钥 {}）", server_url, mask(key));
    let resp = http()
        .post(format!(
            "{}/api/user/info",
            server_url.trim_end_matches('/')
        ))
        .json(&serde_json::json!({"key": key}))
        .send()
        .await
        .map_err(|e| format!("网络错误：无法连接服务器（{e}）"))?;
    if !resp.status().is_success() {
        let status = resp.status();
        let body: serde_json::Value = resp.json().await.unwrap_or_default();
        let msg = body["error"]["message"].as_str().unwrap_or("登录失败");
        tracing::warn!("登录失败 HTTP {status}: {msg}");
        return Err(format!("登录失败（HTTP {status}）：{msg}"));
    }
    let info: AccountInfo = resp
        .json()
        .await
        .map_err(|e| format!("响应解析失败: {e}"))?;

    // 持久化（保留端口偏好等本地设置）
    let mut cfg = load_config();
    cfg.server_url = server_url.trim_end_matches('/').to_string();
    cfg.user = Some(crate::config::StoredUser {
        id: info.id.clone(),
        name: info.name.clone(),
        key: key.to_string(),
    });
    cfg.bound_device = None; // 新登录重置绑定
    save_config(&cfg)?;
    // 显式登录成功 → 后台静默上报设备（失败不影响登录结果）
    tokio::spawn(async move {
        let out = crate::api::report_registration().await;
        if !out.ok {
            tracing::warn!("登录后设备上报未成功（不影响登录）: {:?}", out.error);
        }
    });
    Ok(info)
}

/// 绑定设备（需已登录）：上报本机标识与平台，服务端核验 pctype 一致。
pub async fn bind_device(device_name: &str, pctype: &str, os: &str) -> Result<(), String> {
    let cfg = load_config();
    let Some(user) = cfg.user.clone() else {
        return Err("未登录".into());
    };
    let server = cfg.server_url.clone();
    let os_real = if os.is_empty() {
        pctype.to_string()
    } else {
        os.to_string()
    };
    let resp = http()
        .post(format!("{}/api/device-bind", server))
        .json(&serde_json::json!({
            "key": user.key,
            "userId": user.id,
            "name": device_name,
            "bound": true,
            "pctype": pctype,
            "deviceName": whoami::devicename(),
            "os": os_real,
        }))
        .send()
        .await
        .map_err(|e| format!("网络错误：{e}"))?;
    if !resp.status().is_success() {
        let status = resp.status();
        let body: serde_json::Value = resp.json().await.unwrap_or_default();
        let msg = body["error"]["message"].as_str().unwrap_or("绑定失败");
        return Err(format!("绑定失败（HTTP {status}）：{msg}"));
    }
    let mut cfg = cfg;
    cfg.bound_device = Some(device_name.to_string());
    save_config(&cfg)?;
    Ok(())
}

/// 登出：清 user/bound_device，保留 server_url/port（下次登录省事）。
pub fn logout() -> Result<(), String> {
    let mut cfg: ClientConfig = load_config();
    cfg.user = None;
    cfg.bound_device = None;
    save_config(&cfg)
}

// —— 设备上报注册（POST /api/device-register，key 认证）——

/// 上报请求体（serde camelCase 对齐服务端契约）。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct RegisterBody {
    pub key: String,
    /// 已持久化的设备 ID（首次上报为空串，服务端生成下发）
    pub device_id: String,
    /// 客户端描述（上报覆盖服务端 desc）
    pub desc: String,
    pub port: u16,
    /// 平台：macos / windows
    pub pctype: String,
    /// 主机名
    pub device_name: String,
    /// 与 pctype 一致性由服务端校验（os ≠ pctype → 400）
    pub os: String,
}

/// 上报响应（pending 态 enabled 恒 false）。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct RegisterResult {
    /// approved | pending
    pub status: String,
    pub device_id: String,
    pub enabled: bool,
}

/// 设备上报注册（10s 超时，key 脱敏日志，同现有模式）。
pub async fn register_device(server: &str, body: &RegisterBody) -> Result<RegisterResult, String> {
    tracing::info!("设备上报 {}（密钥 {}）", server, mask(&body.key));
    let resp = http()
        .post(format!(
            "{}/api/device-register",
            server.trim_end_matches('/')
        ))
        .json(body)
        .send()
        .await
        .map_err(|e| format!("网络错误：无法连接服务器（{e}）"))?;
    if !resp.status().is_success() {
        let status = resp.status();
        let body: serde_json::Value = resp.json().await.unwrap_or_default();
        let msg = body["error"]["message"].as_str().unwrap_or("上报失败");
        tracing::warn!("设备上报失败 HTTP {status}: {msg}");
        return Err(format!("上报失败（HTTP {status}）：{msg}"));
    }
    let result: RegisterResult = resp
        .json()
        .await
        .map_err(|e| format!("响应解析失败: {e}"))?;
    tracing::info!(
        "设备上报完成：{}（device {}）",
        result.status,
        result.device_id
    );
    Ok(result)
}
