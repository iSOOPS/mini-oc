//! 隧道地址拼装纯函数。
//!
//! 设备 `public_url` 决定「探测/跳转」地址的形态（继承 V1 运维约定）：
//! 1. **无显式端口**（https 单域名 + nginx 首段端口分流布局）：
//!    探测/代理地址 = `{public_url}/{port}` 路径形态。
//! 2. **带显式端口**（nginx 独立 HTTPS 端口直连）：
//!    探测/代理地址 = `public_url` 原样。
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
/// - public_url 无显式端口：`{public_url}/{port}`
/// - public_url 带显式端口：原样返回
pub fn tunnel_base_url(public_url: &str, port: u16) -> Result<String, TunnelError> {
    let url = parse(public_url)?;
    let base = public_url.trim_end_matches('/');
    if url.port().is_some() {
        Ok(base.to_string())
    } else {
        Ok(format!("{base}/{port}"))
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
