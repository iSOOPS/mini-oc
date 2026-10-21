//! 本地配置持久化：应用数据目录 `config.json`（0600）。
//!
//! `MINI_OC_HOME` env 可覆盖数据目录（测试隔离用）。
//! 默认目录：macOS `~/Library/Application Support/mini-oc`，Windows `%APPDATA%\mini-oc`。

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct StoredUser {
    pub id: String,
    pub name: String,
    pub key: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ClientConfig {
    /// mini-oc-web 服务器地址
    #[serde(default)]
    pub server_url: String,
    #[serde(default)]
    pub user: Option<StoredUser>,
    #[serde(default)]
    pub bound_device: Option<String>,
    /// 纯本地模式（未绑定设备）的端口偏好；云服务以设备清单为准（spec §5.2.1）
    #[serde(default = "default_port")]
    pub port: u16,
    /// opencode 二进制路径覆盖
    #[serde(default)]
    pub opencode_bin: Option<String>,
    /// system | light | dark
    #[serde(default = "default_theme")]
    pub theme: String,
}

fn default_port() -> u16 {
    9464
}

fn default_theme() -> String {
    "system".into()
}

impl Default for ClientConfig {
    fn default() -> Self {
        Self {
            server_url: String::new(),
            user: None,
            bound_device: None,
            port: default_port(),
            opencode_bin: None,
            theme: default_theme(),
        }
    }
}

/// 配置文件路径（MINI_OC_HOME 覆盖 → 平台数据目录）。
pub fn config_path() -> PathBuf {
    let home = std::env::var("MINI_OC_HOME").ok().map(PathBuf::from);
    let dir = home.unwrap_or_else(default_data_dir);
    dir.join("config.json")
}

fn default_data_dir() -> PathBuf {
    #[cfg(target_os = "macos")]
    {
        if let Some(home) = std::env::var_os("HOME") {
            return PathBuf::from(home).join("Library/Application Support/mini-oc");
        }
    }
    #[cfg(target_os = "windows")]
    {
        if let Some(appdata) = std::env::var_os("APPDATA") {
            return PathBuf::from(appdata).join("mini-oc");
        }
    }
    std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")).join("mini-oc-data")
}

/// 读配置：文件缺失/损坏 → 默认值（不 panic）。
pub fn load_config_from(path: &std::path::Path) -> ClientConfig {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .unwrap_or_default()
}

/// 写配置：tempfile + rename 原子替换，0600 权限。
pub fn save_config_to(path: &std::path::Path, cfg: &ClientConfig) -> Result<(), String> {
    let body = serde_json::to_string_pretty(cfg).map_err(|e| format!("序列化失败: {e}"))?;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("创建配置目录失败: {e}"))?;
    }
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, body).map_err(|e| format!("写入配置失败: {e}"))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o600))
            .map_err(|e| format!("设置权限失败: {e}"))?;
    }
    std::fs::rename(&tmp, path).map_err(|e| format!("替换配置失败: {e}"))?;
    Ok(())
}

// —— frb 暴露的无参版本（同步：本地文件操作无 await 必要）—— 

#[flutter_rust_bridge::frb(sync)]
pub fn load_config() -> ClientConfig {
    load_config_from(&config_path())
}

pub fn save_config(cfg: &ClientConfig) -> Result<(), String> {
    save_config_to(&config_path(), cfg)
}
