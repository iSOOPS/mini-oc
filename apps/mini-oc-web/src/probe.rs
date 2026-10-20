//! 设备状态探测：直连设备 opencode v2 `/api/info`（经 rathole 隧道）。
//!
//! 三态判别（spec §6.3）：
//! - 200 → online（附 version）
//! - 401/403 → auth_mismatch（隧道通但凭据不一致——通常是重置过 key）
//! - 超时/连接失败 → offline

use mini_oc_core::device::UserDevice;
use mini_oc_core::ocapi::{OcClient, OcError, OcApi};
use mini_oc_core::tunnel::tunnel_base_url;
use serde::Serialize;

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum DeviceProbe {
    Online { version: String },
    AuthMismatch,
    Offline,
}

impl DeviceProbe {
    pub fn status_str(&self) -> &'static str {
        match self {
            DeviceProbe::Online { .. } => "online",
            DeviceProbe::AuthMismatch => "auth_mismatch",
            DeviceProbe::Offline => "offline",
        }
    }

    pub fn version(&self) -> Option<&str> {
        match self {
            DeviceProbe::Online { version } => Some(version),
            _ => None,
        }
    }
}

/// 探测单台设备（永不返回 Err：探测失败本身就是一种状态）。
pub async fn probe_device(device: &UserDevice, user_key: &str) -> DeviceProbe {
    let base = match tunnel_base_url(&device.public_url, device.port) {
        Ok(b) => b,
        Err(e) => {
            tracing::warn!(device = %device.name, %e, "public_url 非法，判定 offline");
            return DeviceProbe::Offline;
        }
    };
    let client = OcClient::new(&base, user_key);
    match client.info().await {
        Ok(info) => DeviceProbe::Online {
            version: info.version,
        },
        Err(OcError::Unauthorized(_)) => DeviceProbe::AuthMismatch,
        Err(OcError::Transport) => DeviceProbe::Offline,
        Err(e) => {
            tracing::warn!(device = %device.name, %e, "探测异常，判定 offline");
            DeviceProbe::Offline
        }
    }
}
