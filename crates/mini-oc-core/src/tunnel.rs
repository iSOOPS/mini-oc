//! 隧道地址拼装纯函数。
//!
//! `public_url` 是设备的服务器地址（scheme://host，通常不含端口），探测/跳转
//! 地址由「服务器地址 + opencode 端口」拼成**端口形态** `{host}:{port}`（opencode
//! Web UI 需要含端口的干净 origin，不支持路径前缀部署）：
//! - public_url 无显式端口：拼 `{scheme}://{host}:{port}`；
//! - public_url 带显式端口：原样返回（端口已内嵌）。
//!
//! 无论哪种形态，rathole 客户端控制通道地址恒定 `{host}:8999`（单
//! rathole server + 统一控制端口部署），与 public_url 的端口无关。

use thiserror::Error;
use url::Url;

#[derive(Debug, Error, PartialEq)]
pub enum TunnelError {
    #[error("public_url 须为 http(s)://host[:port] 且不含路径: {0}")]
    Invalid(String),
}

/// 解析并校验 public_url：http(s)、有 host、无路径/query/fragment。
fn parse(public_url: &str) -> Result<Url, TunnelError> {
    let url = Url::parse(public_url).map_err(|_| TunnelError::Invalid(public_url.into()))?;
    let ok = (url.scheme() == "http" || url.scheme() == "https")
        && url.host_str().is_some()
        && (url.path().is_empty() || url.path() == "/")
        && url.query().is_none()
        && url.fragment().is_none();
    if !ok {
        return Err(TunnelError::Invalid(public_url.into()));
    }
    Ok(url)
}

/// 设备探测/代理的基础地址。
///
/// - public_url 无显式端口：拼 `{scheme}://{host}:{port}`（端口形态）
/// - public_url 带显式端口：原样返回
pub fn tunnel_base_url(public_url: &str, port: u16) -> Result<String, TunnelError> {
    let url = parse(public_url)?;
    if url.port().is_some() {
        Ok(public_url.trim_end_matches('/').to_string())
    } else {
        // 服务器地址 + opencode 端口 → 端口形态（opencode 需要含端口的干净 origin）
        let scheme = url.scheme();
        let host = url.host_str().expect("parse 已保证 host 存在");
        Ok(format!("{scheme}://{host}:{port}"))
    }
}

/// rathole 客户端配置的 `[client].remote_addr`。
///
/// 当前部署为「单 rathole server + 统一控制端口 8999」：控制通道地址恒定
/// `{host}:8999`，与 public_url 是否带端口无关——public_url 的端口是对外
/// 跳转/探测入口（nginx HTTPS 端口），不是 rathole 控制端口。
pub fn remote_addr_for_rathole(public_url: &str) -> Result<String, TunnelError> {
    let url = parse(public_url)?;
    // url::Url::host_str 对 IPv6 返回带方括号形式（如 "[::1]"），无需再包裹
    let host = url.host_str().expect("parse 已保证 host 存在").to_string();
    Ok(format!("{host}:8999"))
}
