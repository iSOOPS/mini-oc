//! 会话跳转：pair-link 一次性链接（纯函数 + 防开放重定向校验）。

use crate::error::{AppError, AppResult};
use url::Url;

/// 校验 pair-connect URL 与设备隧道 base 同源同前缀（防开放重定向）。
///
/// 三重校验（继承 V1 jump.rs 设计）：
/// 1. 必须是合法 http(s) URL；
/// 2. origin（scheme+host+port）必须与 tunnel base 完全一致；
/// 3. 路径必须以 `/auth/connect/` 开头且恰好一段 code。
pub fn validate_jump_url(candidate: &str, tunnel_base: &str) -> AppResult<()> {
    let parsed: Url = Url::parse(candidate)
        .map_err(|_| AppError::PairUrlInvalid(format!("无法解析: {candidate}")))?;
    let base: Url = Url::parse(tunnel_base)
        .map_err(|_| AppError::Internal(format!("隧道 base 非法: {tunnel_base}")))?;

    let origin_ok = parsed.scheme() == base.scheme()
        && parsed.host_str() == base.host_str()
        && parsed.port() == base.port();
    if !origin_ok {
        return Err(AppError::PairUrlInvalid(format!(
            "跳转 URL 与设备隧道不同源: {candidate} vs {tunnel_base}"
        )));
    }
    // 路径校验：tunnel base 可能带 /{port} 前缀（路径形态），connect 路径必须紧跟其后
    let base_path = base.path().trim_end_matches('/');
    let expected_prefix = format!("{base_path}/auth/connect/");
    if !parsed.path().starts_with(&expected_prefix) {
        return Err(AppError::PairUrlInvalid(format!(
            "跳转路径前缀非法: {}（期望 {}）",
            parsed.path(),
            expected_prefix
        )));
    }
    let code = parsed
        .path()
        .strip_prefix(&expected_prefix)
        .unwrap_or_default();
    if code.is_empty() || code.contains('/') {
        return Err(AppError::PairUrlInvalid("code 为空或含多余路径段".into()));
    }
    Ok(())
}

/// 拼装 connect URL。
pub fn connect_url(tunnel_base: &str, code: &str) -> String {
    format!(
        "{}/auth/connect/{code}",
        tunnel_base.trim_end_matches('/')
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validate_same_origin_ok_both_forms() {
        // 路径形态
        assert!(validate_jump_url(
            "https://oc.example.com/9464/auth/connect/abc",
            "https://oc.example.com/9464"
        )
        .is_ok());
        // 直连端口形态
        assert!(validate_jump_url(
            "http://1.2.3.4:9101/auth/connect/abc",
            "http://1.2.3.4:9101"
        )
        .is_ok());
    }

    #[test]
    fn validate_rejects_cross_origin_and_bad_paths() {
        let base = "https://oc.example.com/9464";
        assert!(validate_jump_url("https://evil.com/9464/auth/connect/abc", base).is_err());
        assert!(validate_jump_url("https://oc.example.com:9999/auth/connect/abc", base).is_err());
        assert!(validate_jump_url("https://oc.example.com/9464/other/abc", base).is_err());
        assert!(validate_jump_url("https://oc.example.com/9464/auth/connect/", base).is_err());
        assert!(validate_jump_url("https://oc.example.com/9464/auth/connect/a/b", base).is_err());
    }
}
