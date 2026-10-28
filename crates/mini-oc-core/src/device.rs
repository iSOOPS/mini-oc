//! 用户/设备数据模型与 V1→V2 注册表迁移。
//!
//! V2 关键变化（相对 V1 mini-oc-web）：
//! - 设备 `port` 语义收敛为「设备端 opencode serve 端口」（V1 的 TUI axum 端口已废弃）
//! - 删除用户级 SB 配置（`sb`）与 `cloud_ip`
//! - serde 统一 camelCase（前端 TS 友好；SB md 中存储格式随之改变，schema version = 2）

use rand::Rng;
use serde::{Deserialize, Serialize};

/// serde 缺省值：true（`enabled` 缺省必须为 true，否则存量设备会被批量解读为停用）。
fn default_true() -> bool {
    true
}

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
    /// 启用状态：false = 停用（不下发、不探测、不可 jump）。
    /// 缺省解读为 true（存量设备不受影响）。
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// 设备唯一 ID（UUID，服务端首次上报时生成）；旧数据为空串，客户端重新上报时回填。
    #[serde(default)]
    pub device_id: String,
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

/// 待审核设备（客户端主动上报、管理员审核前暂存）。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PendingDevice {
    /// UUID，服务端生成
    pub device_id: String,
    /// 客户端上报的描述（可空串）
    pub desc: String,
    /// 客户端 opencode 端口
    pub port: u16,
    /// 平台：macos / windows
    pub pctype: String,
    /// 主机名
    pub device_name: String,
    /// RFC3339 最近一次上报时间
    pub reported_at: String,
}

impl PendingDevice {
    /// 字段白名单校验，返回 Err(原因说明)。
    pub fn validate(&self) -> Result<(), String> {
        if self.port == 0 {
            return Err("port 须大于 0".into());
        }
        if self.pctype != "macos" && self.pctype != "windows" {
            return Err("pctype 须为 macos 或 windows".into());
        }
        let name_len = self.device_name.chars().count();
        if !(1..=128).contains(&name_len) {
            return Err("device_name 长度须为 1-128 字".into());
        }
        if self.desc.chars().count() > 64 {
            return Err("desc 长度须不超过 64 字".into());
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
    /// 待审核设备（客户端上报、管理员审核前的暂存区）
    #[serde(default)]
    pub pending_devices: Vec<PendingDevice>,
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

/// V1 设备条目（wire 格式：**kebab-case** 字段，与 V1 mini-oc-web users.rs 的
/// serde 定义逐字段对齐；snake_case 作为别名兼容本仓库早期误写）。
/// 裸字符串是更早的 legacy 设备形态（仅服务名）。
#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum V1DeviceWire {
    Full {
        #[serde(default, alias = "d-name")]
        desc: String,
        name: String,
        #[serde(default = "v1_legacy_default_port")]
        port: u16,
        /// V1 的 opencode serve 端口——V2 `port` 的语义本体
        #[serde(rename = "oc-port", alias = "oc_port", default)]
        oc_port: Option<u16>,
        #[serde(rename = "device-name", alias = "device_name", default)]
        device_name: String,
        #[serde(default)]
        pctype: String,
        #[serde(default)]
        bound: bool,
        #[serde(rename = "public-url", alias = "public_url", default)]
        public_url: String,
    },
    /// 更早的 legacy 形态：裸服务名字符串（pctype/public_url 全空，不可用）
    Legacy(String),
}

/// V1 的 legacy 设备缺省端口（users.rs LEGACY_DEFAULT_PORT）。
fn v1_legacy_default_port() -> u16 {
    4040
}

#[derive(Debug, Deserialize)]
struct V1User {
    id: String,
    name: String,
    key: String,
    #[serde(default)]
    devices: Vec<V1DeviceWire>,
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
/// - 设备字段兼容 kebab-case（V1 真实格式）与 snake-case（本仓库早期误写）双形态；
/// - 设备 port 取 V1 `oc-port`（opencode serve 端口），缺失时保留原 `port`（含 legacy 默认 4040）；
/// - 剥除 cloud_ip / sb / oc-port 等废弃字段；
/// - 裸字符串 legacy 设备（pctype/public_url 全空、不可探测）跳过并告警；
/// - **UUID 形态的旧 id 改写为新的唯一 6 位数字**（保留用户不丢弃，对齐 V1 迁移策略）；
/// - 单条设备校验失败跳过该设备（不炸全表）。
pub fn migrate_v1(raw: &str) -> Result<UsersFile, String> {
    let v1: V1File = serde_json::from_str(raw).map_err(|e| format!("V1 注册表解析失败: {e}"))?;
    // 第一遍：收集已占用的 6 位 id（原样保留的用户）
    let mut taken: Vec<String> = v1
        .users
        .iter()
        .filter(|u| u.id.len() == 6 && u.id.chars().all(|c| c.is_ascii_digit()))
        .map(|u| u.id.clone())
        .collect();

    let mut users = Vec::new();
    for u in v1.users {
        let mut devices = Vec::new();
        for d in u.devices {
            let dev = match d {
                V1DeviceWire::Full {
                    desc,
                    name,
                    port,
                    oc_port,
                    device_name,
                    pctype,
                    bound,
                    public_url,
                } => UserDevice {
                    desc,
                    name,
                    port: oc_port.unwrap_or(port),
                    pctype,
                    device_name,
                    bound,
                    public_url,
                    enabled: true,
                    device_id: String::new(),
                },
                V1DeviceWire::Legacy(name) => {
                    tracing::warn!(user = %u.id, device = %name, "跳过裸字符串 legacy 设备（信息不全，不可用）");
                    continue;
                }
            };
            match dev.validate() {
                Ok(()) => devices.push(dev),
                Err(reason) => {
                    tracing::warn!(user = %u.id, device = %dev.name, %reason, "迁移跳过非法设备条目");
                }
            }
        }
        // UUID 旧 id → 新 6 位数字（不丢弃用户）
        let id = if u.id.len() == 6 && u.id.chars().all(|c| c.is_ascii_digit()) {
            u.id
        } else {
            let new_id = generate_user_id(&taken);
            tracing::warn!(old = %u.id, new = %new_id, "UUID 形态旧 id 改写为 6 位数字");
            taken.push(new_id.clone());
            new_id
        };
        let user = PortalUser {
            id,
            name: u.name,
            key: u.key,
            devices,
            pending_devices: vec![],
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

/// 生成设备唯一 ID（RFC 4122 v4 UUID 格式）。
///
/// 不引入 `uuid` crate：16 字节随机 + 版本位（4）与 RFC 变体位，连字符格式化。
pub fn generate_device_id() -> String {
    let mut bytes: [u8; 16] = rand::random();
    // 版本位：高 4 字节的高 4 bit 置为 0100（v4）
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    // 变体位：RFC 4122，高 2 bit 置为 10
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    format!(
        "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
        bytes[8], bytes[9], bytes[10], bytes[11], bytes[12], bytes[13], bytes[14], bytes[15]
    )
}
