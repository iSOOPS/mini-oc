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

/// 服务端探测/列表/pair 的**首选**基址（单路径场景：项目/会话、pair）。
///
/// 门户与 rathole server 常同机部署：主动调用优先走**内网 rathole 直连**
/// （`RATHOLE_PROBE_BASE`，如 `http://rathole`），未配置时退回 public_url 端口形态。
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

/// 探测候选基址（按优先级）：内网 rathole 优先 → 公网 public_url 兜底。
///
/// 状态探测按顺序回退：内网不通（容器间 DNS/网络异常、客户端未注册该 service 等）
/// 时仍有机会经公网命中；`public_url` 非法则仅保留内网候选。
pub fn probe_bases(internal_base: &str, public_url: &str, port: u16) -> Vec<String> {
    let mut bases = Vec::new();
    if let Ok(primary) = resolve_probe_base(internal_base, public_url, port) {
        bases.push(primary);
    }
    if let Ok(pub_base) = tunnel_base_url(public_url, port) {
        if !bases.contains(&pub_base) {
            bases.push(pub_base);
        }
    }
    bases
}

/// 探测单台设备（永不返回 Err：探测失败本身就是一种状态）。
///
/// 多路径回退：依次尝试候选基址，任一 200 即 online；任一 401/403 直接判
/// auth_mismatch（凭据一致性问题与路径无关，无需再试）；全部连接失败才 offline。
pub async fn probe_device(device: &UserDevice, user_key: &str, internal_base: &str) -> DeviceProbe {
    let bases = probe_bases(internal_base, &device.public_url, device.port);
    if bases.is_empty() {
        tracing::warn!(device = %device.name, "public_url 非法且无内网基址，判定 offline");
        return DeviceProbe::Offline;
    }

    let mut last_err: Option<OcError> = None;
    for base in &bases {
        let client = OcClient::new(base, user_key);
        match client.info().await {
            Ok(info) => {
                tracing::info!(device = %device.name, base = %base, "设备在线");
                return DeviceProbe::Online {
                    version: info.version,
                };
            }
            Err(OcError::Unauthorized(code)) => {
                tracing::warn!(device = %device.name, base = %base, code, "鉴权不一致");
                return DeviceProbe::AuthMismatch;
            }
            Err(e) => {
                tracing::warn!(device = %device.name, base = %base, error = %e, "该探测路径失败，尝试下一条");
                last_err = Some(e);
            }
        }
    }

    tracing::warn!(
        device = %device.name,
        bases = ?bases,
        last_error = ?last_err.map(|e| e.to_string()),
        "全部探测路径失败，判定 offline"
    );
    DeviceProbe::Offline
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

    #[test]
    fn probe_bases_internal_then_public() {
        // 内网优先 + 公网兜底（去重）
        assert_eq!(
            probe_bases("http://rathole", "https://oc.example.com", 9000),
            vec!["http://rathole:9000".to_string(), "https://oc.example.com:9000".to_string()]
        );
        // 内网为空 → 仅公网兜底
        assert_eq!(
            probe_bases("", "https://oc.example.com", 9464),
            vec!["https://oc.example.com:9464".to_string()]
        );
        // 内网与公网同基址 → 去重
        assert_eq!(
            probe_bases("", "http://rathole:9000", 9000),
            vec!["http://rathole:9000".to_string()]
        );
        // public_url 非法 → 仅内网候选
        assert_eq!(
            probe_bases("http://rathole", "ftp://x.com", 9464),
            vec!["http://rathole:9464".to_string()]
        );
    }
}
