//! 会话跳转：pair-link 一次性链接（纯函数 + 防开放重定向校验）。

use crate::error::{AppError, AppResult};
use base64::engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD};
use base64::Engine;
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

/// 提取隧道 base 的 origin（scheme://host[:port]）。
///
/// opencode v2 Web UI 用 `location.origin`（不含路径）作为当前 server 的
/// url 并据此生成 serverKey；路径形态隧道（`{host}/{port}`）的 origin 须剥离
/// 端口路径前缀，否则 serverKey 对不上。
pub fn tunnel_origin(tunnel_base: &str) -> AppResult<String> {
    let url = Url::parse(tunnel_base)
        .map_err(|_| AppError::Internal(format!("隧道 base 非法: {tunnel_base}")))?;
    Ok(url.origin().ascii_serialization())
}

/// 构造会话深链：`{tunnel}/server/{serverKey}/session/{id}?auth_token={base64(opencode:token)}`。
///
/// 依据（对 opencode 2.2.2 实测 + 二进制源码确认）：
/// - Web UI 路由为 `/server/:serverKey/session/:id`；
/// - `serverKey = base64url(location.origin)`（无 padding，`+`→`-`、`/`→`_`）；
/// - `auth_token = base64("opencode:" + token)`，服务端中间件在入口消费后
///   自动从 URL 移除。
///
/// session/token 做白名单校验，防路径注入（开放重定向）。
pub fn session_deep_link(tunnel_base: &str, session: &str, token: &str) -> AppResult<String> {
    // 会话 ID 实测形如 ses_xxx（含 `_`）；pair token 为 exp.hmac（含 `.`）
    if session.is_empty()
        || !session
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
    {
        return Err(AppError::PairUrlInvalid(format!("会话 ID 非法: {session}")));
    }
    if token.is_empty()
        || !token
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-' || c == '.')
    {
        return Err(AppError::PairUrlInvalid("pair token 非法".into()));
    }

    let origin = tunnel_origin(tunnel_base)?;
    let server_key = URL_SAFE_NO_PAD.encode(origin.as_bytes());
    let auth = STANDARD.encode(format!("opencode:{token}").as_bytes());
    let mut url = Url::parse(&format!(
        "{}/server/{}/session/{session}",
        tunnel_base.trim_end_matches('/'),
        server_key
    ))
    .map_err(|_| AppError::Internal("深链拼装失败".into()))?;
    // query_pairs_mut 对 base64 标准字符（+/=）做 percent-encoding，避免 `+` 被
    // 服务端 URLSearchParams 解析为空格。
    url.query_pairs_mut().append_pair("auth_token", &auth);
    Ok(url.to_string())
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

    #[test]
    fn tunnel_origin_strips_path_prefix() {
        // 端口形态：origin 即 tunnel 本身
        assert_eq!(
            tunnel_origin("http://1.2.3.4:9101").unwrap(),
            "http://1.2.3.4:9101"
        );
        // 路径形态：剥离 /{port} 前缀（opencode Web UI 的 location.origin 不含路径）
        assert_eq!(
            tunnel_origin("https://oc.example.com/9464").unwrap(),
            "https://oc.example.com"
        );
    }

    #[test]
    fn session_deep_link_encodes_server_key_and_auth_token() {
        let url = session_deep_link(
            "https://oc.example.com/9464",
            "ses_abc123",
            "1796025749.EKzkuAmKP9wfB4QAWNYZNKlPddvzWv9ruxIYnWJ4ZHg",
        )
        .unwrap();
        // serverKey = base64url("https://oc.example.com")；auth_token = base64("opencode:<token>")
        assert_eq!(
            url,
            "https://oc.example.com/9464/server/aHR0cHM6Ly9vYy5leGFtcGxlLmNvbQ/session/ses_abc123?auth_token=b3BlbmNvZGU6MTc5NjAyNTc0OS5FS3prdUFtS1A5d2ZCNFFBV05ZWk5LbFBkZHZ6V3Y5cnV4SVluV0o0Wkhn"
        );
    }

    #[test]
    fn session_deep_link_port_form() {
        let url = session_deep_link(
            "http://1.2.3.4:9101",
            "ses_xyz",
            "1796025749.EKzkuAmKP9wfB4QAWNYZNKlPddvzWv9ruxIYnWJ4ZHg",
        )
        .unwrap();
        assert_eq!(
            url,
            "http://1.2.3.4:9101/server/aHR0cDovLzEuMi4zLjQ6OTEwMQ/session/ses_xyz?auth_token=b3BlbmNvZGU6MTc5NjAyNTc0OS5FS3prdUFtS1A5d2ZCNFFBV05ZWk5LbFBkZHZ6V3Y5cnV4SVluV0o0Wkhn"
        );
    }

    #[test]
    fn session_deep_link_rejects_unsafe_inputs() {
        let token = "1796025749.EKzkuAmKP9wfB4QAWNYZNKlPddvzWv9ruxIYnWJ4ZHg";
        // session id 含路径分隔符 → 拒绝
        assert!(session_deep_link("http://1.2.3.4:9101", "ses/../evil", token).is_err());
        // token 含空格/非法字符 → 拒绝
        assert!(session_deep_link("http://1.2.3.4:9101", "ses_ok", "bad token").is_err());
        assert!(session_deep_link("http://1.2.3.4:9101", "ses_ok", "a/b?c=d").is_err());
    }
}
