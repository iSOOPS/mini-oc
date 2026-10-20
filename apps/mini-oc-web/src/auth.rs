//! HMAC 签名 cookie 与登录限流（自 V1 移植，已验证的安全逻辑）。

use crate::error::{AppError, AppResult};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use hmac::{Hmac, Mac};
use sha2::Sha256;
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

type HmacSha256 = Hmac<Sha256>;

/// HMAC-SHA256 签名 cookie 值：`base64url(subject|ttl).base64url(signature)`
pub fn sign_cookie(subject: &str, key: &[u8], ttl: Duration) -> AppResult<String> {
    let mut mac = HmacSha256::new_from_slice(key)
        .map_err(|e| AppError::Internal(format!("hmac key error: {e}")))?;
    let payload = format!("{subject}|{}", ttl.as_secs());
    mac.update(payload.as_bytes());
    let sig = URL_SAFE_NO_PAD.encode(mac.finalize().into_bytes());
    let payload_b64 = URL_SAFE_NO_PAD.encode(payload.as_bytes());
    Ok(format!("{payload_b64}.{sig}"))
}

pub fn verify_cookie(cookie: &str, key: &[u8]) -> AppResult<String> {
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
    if inner.len() != 2 {
        return Err(AppError::Unauthorized("bad payload format".into()));
    }
    let subject = inner[0];
    let ttl_secs: u64 = inner[1]
        .parse()
        .map_err(|_| AppError::Unauthorized("bad ttl".into()))?;
    let sig = URL_SAFE_NO_PAD
        .decode(parts[1])
        .map_err(|_| AppError::Unauthorized("bad sig encoding".into()))?;
    let mut mac = HmacSha256::new_from_slice(key)
        .map_err(|e| AppError::Internal(format!("hmac key error: {e}")))?;
    mac.update(payload.as_bytes());
    mac.verify_slice(&sig)
        .map_err(|_| AppError::Unauthorized("bad signature".into()))?;
    if ttl_secs == 0 {
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
