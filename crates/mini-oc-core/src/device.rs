//! 用户/设备数据模型与 V1→V2 注册表迁移。
//!
//! V2 关键变化（相对 V1 mini-oc-web）：
//! - 设备 `port` 语义收敛为「设备端 opencode serve 端口」（V1 的 TUI axum 端口已废弃）
//! - 删除用户级 SB 配置（`sb`）与 `cloud_ip`
//! - serde 统一 camelCase（前端 TS 友好；SB md 中存储格式随之改变，schema version = 2）

use rand::Rng;
use serde::{Deserialize, Serialize};

/// 单台用户设备（对应一条 rathole 隧道 + 设备端 opencode serve）。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct UserDevice {
    /// 描述（展示用，1-64 字）
    pub desc: String,
    /// rathole 服务名 ^[a-z0-9-]{1,64}$（全局唯一键）
    pub name: String,
    /// 设备端 opencode serve 端口（rathole 转发的本地端口）
    pub port: u16,
    /// 平台类型：windows | macos
    pub pctype: String,
    /// 主机名（客户端绑定时写入，只读展示）
    #[serde(default)]
    pub device_name: String,
    /// 绑定状态（客户端写入）
    #[serde(default)]
    pub bound: bool,
    /// 隧道公网地址 scheme://host[:port]（超管录入）
    pub public_url: String,
}

impl UserDevice {
    /// 字段白名单校验，返回 Err(原因说明)。
    pub fn validate(&self) -> Result<(), String> {
        if self.desc.is_empty() || self.desc.chars().count() > 64 {
            return Err("desc 长度须为 1-64 字".into());
        }
        let name_ok = (1..=64).contains(&self.name.len())
            && self.name.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-');
        if !name_ok {
            return Err("name 须匹配 ^[a-z0-9-]{1,64}$".into());
        }
        if self.pctype != "windows" && self.pctype != "macos" {
            return Err("pctype 须为 windows 或 macos".into());
        }
        if !valid_public_url(&self.public_url) {
            return Err("public_url 须为 http(s)://host[:port] 且不含路径".into());
        }
        Ok(())
    }
}

/// public_url 校验：http(s) 协议、有 host、无路径。
fn valid_public_url(raw: &str) -> bool {
    let Ok(url) = url::Url::parse(raw) else {
        return false;
    };
    if url.scheme() != "http" && url.scheme() != "https" {
        return false;
    }
    if url.host_str().is_none() {
        return false;
    }
    // path 仅允许空或 "/"
    if url.path() != "/" && !url.path().is_empty() {
        return false;
    }
    // 不允许 query/fragment
    url.query().is_none() && url.fragment().is_none()
}

/// 门户用户。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PortalUser {
    /// 6 位数字编号
    pub id: String,
    /// 用户名 ^[A-Za-z0-9_-]{1,64}$
    pub name: String,
    /// 32 位密钥（全链路唯一凭据）
    pub key: String,
    pub devices: Vec<UserDevice>,
    pub created_at: String,
    pub updated_at: String,
    pub last_used_at: Option<String>,
}

impl PortalUser {
    pub fn validate(&self) -> Result<(), String> {
        if self.id.len() != 6 || !self.id.chars().all(|c| c.is_ascii_digit()) {
            return Err("id 须为 6 位数字".into());
        }
        let name_ok = (1..=64).contains(&self.name.len())
            && self
                .name
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-');
        if !name_ok {
            return Err("name 须匹配 ^[A-Za-z0-9_-]{1,64}$".into());
        }
        if self.key.len() != 32 || !self.key.chars().all(|c| c.is_ascii_alphanumeric()) {
            return Err("key 须为 32 位字母数字".into());
        }
        for d in &self.devices {
            d.validate()?;
        }
        Ok(())
    }
}

/// 注册表文件结构（SB md 内容的 JSON 主体）。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct UsersFile {
    /// schema 版本（当前 2；web 层读取判断是否需要迁移）
    #[allow(dead_code)]
    pub version: u32,
    pub users: Vec<PortalUser>,
}

// ---------------------------------------------------------------------------
// V1 → V2 迁移
// ---------------------------------------------------------------------------

/// V1 设备条目（含废弃字段，宽容反序列化）。
#[derive(Debug, Deserialize)]
struct V1Device {
    desc: String,
    name: String,
    port: u16,
    /// V1 的 opencode serve 端口——V2 `port` 的语义本体
    #[serde(default)]
    oc_port: Option<u16>,
    #[serde(default)]
    device_name: String,
    pctype: String,
    #[serde(default)]
    bound: bool,
    public_url: String,
}

/// V1 用户条目（含废弃字段）。
#[derive(Debug, Deserialize)]
struct V1User {
    id: String,
    name: String,
    key: String,
    #[serde(default)]
    devices: Vec<V1Device>,
    created_at: String,
    updated_at: String,
    #[serde(default)]
    last_used_at: Option<String>,
}

#[derive(Debug, Deserialize)]
struct V1File {
    /// version 字段（1 或缺失均按 V1 处理）被 serde 忽略；
    /// V1/V2 格式区分由调用方（SbClient::get_users）先试 V2 反序列化、失败再走 migrate_v1 完成。
    #[serde(default)]
    users: Vec<V1User>,
}

/// V1 注册表 JSON → V2 UsersFile。
///
/// 规则：
/// - 设备 port 取 V1 `ocPort`（opencode serve 端口），缺失时保留原 `port`；
/// - 剥除 cloudIp / sb / ocPort 等废弃字段；
/// - 单条设备校验失败跳过该设备（不炸全表）；
/// - 整条用户结构性损坏跳过该用户。
pub fn migrate_v1(raw: &str) -> Result<UsersFile, String> {
    let v1: V1File = serde_json::from_str(raw).map_err(|e| format!("V1 注册表解析失败: {e}"))?;
    let mut users = Vec::new();
    for u in v1.users {
        let mut devices = Vec::new();
        for d in u.devices {
            let port = d.oc_port.unwrap_or(d.port);
            let dev = UserDevice {
                desc: d.desc,
                name: d.name,
                port,
                pctype: d.pctype,
                device_name: d.device_name,
                bound: d.bound,
                public_url: d.public_url,
            };
            match dev.validate() {
                Ok(()) => devices.push(dev),
                Err(reason) => {
                    tracing::warn!(user = %u.id, device = %dev.name, %reason, "迁移跳过非法设备条目");
                }
            }
        }
        let user = PortalUser {
            id: u.id,
            name: u.name,
            key: u.key,
            devices,
            created_at: u.created_at,
            updated_at: u.updated_at,
            last_used_at: u.last_used_at,
        };
        match user.validate() {
            Ok(()) => users.push(user),
            Err(reason) => {
                tracing::warn!(user = %user.id, %reason, "迁移跳过非法用户条目");
            }
        }
    }
    Ok(UsersFile { version: 2, users })
}

// ---------------------------------------------------------------------------
// 生成器
// ---------------------------------------------------------------------------

/// 生成不与现有冲突的 6 位数字用户编号。
pub fn generate_user_id(existing: &[String]) -> String {
    let mut rng = rand::thread_rng();
    loop {
        let id: u32 = rng.gen_range(100_000..1_000_000);
        let id = id.to_string();
        if !existing.contains(&id) {
            return id;
        }
    }
}

/// 生成 32 位字母数字密钥。
pub fn generate_key() -> String {
    const CHARSET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789";
    let mut rng = rand::thread_rng();
    (0..32)
        .map(|_| CHARSET[rng.gen_range(0..CHARSET.len())] as char)
        .collect()
}
