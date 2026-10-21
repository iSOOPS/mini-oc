//! HMAC 签名 cookie 与登录限流（自 V1 移植，已验证的安全逻辑）。

use crate::error::{AppError, AppResult};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use hmac::{Hmac, Mac};
use sha2::Sha256;
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

type HmacSha256 = Hmac<Sha256>;

/// HMAC-SHA256 签名 cookie：`base64url(subject|ttl|issued_at).base64url(sig)`
pub fn sign_cookie(subject: &str, key: &[u8], ttl: Duration) -> AppResult<String> {
    sign_cookie_at(subject, key, ttl, chrono::Utc::now().timestamp())
}

/// 带签发时间的签名（服务端 TTL 校验依赖 issued_at，I1 修复）。
pub fn sign_cookie_at(subject: &str, key: &[u8], ttl: Duration, now_secs: i64) -> AppResult<String> {
    let mut mac = HmacSha256::new_from_slice(key)
        .map_err(|e| AppError::Internal(format!("hmac key error: {e}")))?;
    let payload = format!("{subject}|{}|{now_secs}", ttl.as_secs());
    mac.update(payload.as_bytes());
    let sig = URL_SAFE_NO_PAD.encode(mac.finalize().into_bytes());
    let payload_b64 = URL_SAFE_NO_PAD.encode(payload.as_bytes());
    Ok(format!("{payload_b64}.{sig}"))
}

pub fn verify_cookie(cookie: &str, key: &[u8]) -> AppResult<String> {
    verify_cookie_at(cookie, key, chrono::Utc::now().timestamp())
}

/// 带当前时间的校验：签名 + TTL 服务端强制过期。
pub fn verify_cookie_at(cookie: &str, key: &[u8], now_secs: i64) -> AppResult<String> {
    let parts: Vec<&str> = cookie.split('.').collect();
    if parts.len() != 2 {
        return Err(AppError::Unauthorized("malformed cookie".into()));
    }
    let payload_bytes = URL_SAFE_NO_PAD
        .decode(parts[0])
        .map_err(|_| AppError::Unauthorized("bad payload encoding".into()))?;
    let payload = String::from_utf8(payload_bytes)
        .map_err(|_| AppError::Unauthorized("bad payload utf8".into()))?;
    let inner: Vec<&str> = payload.split('|').collect();
    if inner.len() != 3 {
        // 兼容无签发时间的旧格式：一律拒绝（要求重新登录，安全优先）
        return Err(AppError::Unauthorized("expired".into()));
    }
    let subject = inner[0];
    let ttl_secs: i64 = inner[1]
        .parse()
        .map_err(|_| AppError::Unauthorized("bad ttl".into()))?;
    let issued_at: i64 = inner[2]
        .parse()
        .map_err(|_| AppError::Unauthorized("bad issued_at".into()))?;
    let sig = URL_SAFE_NO_PAD
        .decode(parts[1])
        .map_err(|_| AppError::Unauthorized("bad sig encoding".into()))?;
    let mut mac = HmacSha256::new_from_slice(key)
        .map_err(|e| AppError::Internal(format!("hmac key error: {e}")))?;
    mac.update(payload.as_bytes());
    mac.verify_slice(&sig)
        .map_err(|_| AppError::Unauthorized("bad signature".into()))?;
    // 服务端 TTL 强制过期（不再依赖浏览器 Max-Age）
    if ttl_secs <= 0 || now_secs.saturating_sub(issued_at) >= ttl_secs {
        return Err(AppError::Unauthorized("expired".into()));
    }
    Ok(subject.to_string())
}

/// 内存 per-IP 限流器：窗口内达到 limit 次即拒绝。
#[derive(Debug)]
pub struct RateLimiter {
    limit: u32,
    window: Duration,
    state: Mutex<HashMap<String, Vec<Instant>>>,
}

impl RateLimiter {
    pub fn new(limit: u32, window: Duration) -> Self {
        Self {
            limit,
            window,
            state: Mutex::new(HashMap::new()),
        }
    }

    pub async fn check(&self, ip: &str) -> AppResult<()> {
        let now = Instant::now();
        let mut state = self.state.lock().unwrap();
        let entries = state.entry(ip.to_string()).or_default();
        entries.retain(|t| now.duration_since(*t) < self.window);
        if entries.len() >= self.limit as usize {
            Err(AppError::RateLimited)
        } else {
            Ok(())
        }
    }

    pub async fn record_failure(&self, ip: &str) {
        let now = Instant::now();
        let mut state = self.state.lock().unwrap();
        let entries = state.entry(ip.to_string()).or_default();
        entries.retain(|t| now.duration_since(*t) < self.window);
        entries.push(now);
    }

    pub async fn reset(&self, ip: &str) {
        self.state.lock().unwrap().remove(ip);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cookie_sign_verify_roundtrip() {
        let key = b"0123456789abcdef0123456789abcdef";
        let cookie = sign_cookie("u:100001", key, Duration::from_secs(3600)).unwrap();
        assert_eq!(verify_cookie(&cookie, key).unwrap(), "u:100001");
    }

    #[test]
    fn cookie_expires_server_side() {
        // I1 修复：TTL 必须由服务端强制——签发于 2 小时前、TTL 1 小时的 cookie 拒绝
        let key = b"0123456789abcdef0123456789abcdef";
        let now = 1_800_000_000i64;
        let cookie = sign_cookie_at("admin", key, Duration::from_secs(3600), now - 7200).unwrap();
        let err = verify_cookie_at(&cookie, key, now).unwrap_err();
        assert!(err.to_string().contains("expired"), "got {err}");
        // 未过期仍有效
        let fresh = sign_cookie_at("admin", key, Duration::from_secs(3600), now - 60).unwrap();
        assert_eq!(verify_cookie_at(&fresh, key, now).unwrap(), "admin");
    }

    #[test]
    fn cookie_tamper_rejected() {
        let key = b"0123456789abcdef0123456789abcdef";
        let cookie = sign_cookie("u:100001", key, Duration::from_secs(3600)).unwrap();
        let other_key = b"ffffffffffffffffffffffffffffffff";
        assert!(verify_cookie(&cookie, other_key).is_err());
        assert!(verify_cookie("garbage", key).is_err());
    }

    #[tokio::test]
    async fn rate_limiter_blocks_at_limit() {
        let limiter = RateLimiter::new(3, Duration::from_secs(60));
        for _ in 0..3 {
            limiter.record_failure("1.2.3.4").await;
        }
        assert!(limiter.check("1.2.3.4").await.is_err());
        assert!(limiter.check("5.6.7.8").await.is_ok(), "不影响其他 IP");
        limiter.reset("1.2.3.4").await;
        assert!(limiter.check("1.2.3.4").await.is_ok());
    }
}
