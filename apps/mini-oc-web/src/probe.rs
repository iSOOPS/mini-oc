//! 设备状态探测：直连设备 opencode v2 `/api/info`（经 rathole 隧道）。
//!
//! 三态判别（spec §6.3）：
//! - 200 → online（附 version）
//! - 401/403 → auth_mismatch（隧道通但凭据不一致——通常是重置过 key）
//! - 超时/连接失败 → offline

use mini_oc_core::device::UserDevice;
use mini_oc_core::ocapi::{OcClient, OcError, OcApi};
use mini_oc_core::tunnel::{tunnel_base_url, TunnelError};
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

/// 服务端探测/列表/pair 的基础地址。
///
/// 门户与 rathole server 常同机部署：从 Docker 容器内直连「自己公网 IP 的设备端口段」
/// 会因安全组/自访回环限制而失败（这也是设备显示「未启动服务」的根因）。故服务端
/// 主动发起的调用应走**内网 rathole 直连**（`RATHOLE_PROBE_BASE`，如 `http://rathole`）；
/// 未配置时退回 public_url 端口形态（本地开发/单机测试）。
pub fn resolve_probe_base(
    internal_base: &str,
    public_url: &str,
    port: u16,
) -> Result<String, TunnelError> {
    let b = internal_base.trim().trim_end_matches('/');
    if b.is_empty() {
        tunnel_base_url(public_url, port)
    } else {
        Ok(format!("{b}:{port}"))
    }
}

/// 探测单台设备（永不返回 Err：探测失败本身就是一种状态）。
pub async fn probe_device(device: &UserDevice, user_key: &str, internal_base: &str) -> DeviceProbe {
    let base = match resolve_probe_base(internal_base, &device.public_url, device.port) {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_probe_base_prefers_internal() {
        // 配置了内网 rathole 基址 → 内网直连（带端口，无 TLS）
        assert_eq!(
            resolve_probe_base("http://rathole", "https://oc.example.com", 9000).unwrap(),
            "http://rathole:9000"
        );
        // 基址带尾斜杠 → 去掉
        assert_eq!(
            resolve_probe_base("http://rathole/", "https://oc.example.com", 9000).unwrap(),
            "http://rathole:9000"
        );
    }

    #[test]
    fn resolve_probe_base_falls_back_to_public_when_empty() {
        // 内网基址为空 → 退回 public_url 端口形态（本地开发/单机测试）
        assert_eq!(
            resolve_probe_base("", "https://oc.example.com", 9464).unwrap(),
            "https://oc.example.com:9464"
        );
        // 空白串同样退回
        assert_eq!(
            resolve_probe_base("   ", "http://1.2.3.4", 9001).unwrap(),
            "http://1.2.3.4:9001"
        );
    }

    #[test]
    fn resolve_probe_base_invalid_public_url() {
        // 内网基址为空且 public_url 非法 → 报错（同 tunnel_base_url 语义）
        assert!(resolve_probe_base("", "ftp://x.com", 9464).is_err());
    }
}
