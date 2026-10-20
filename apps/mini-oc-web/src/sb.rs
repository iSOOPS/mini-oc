//! SilverBullet 客户端（自 V1 移植登录/重试逻辑，新增 UsersFile 语义）。
//!
//! 认证协议：POST `/.auth` form 登录 → 内存 cookie（名字按 host 派生）；
//! `/.fs/*` 401 自动重登一次重试。

use crate::error::{AppError, AppResult};
use mini_oc_core::device::{migrate_v1, UsersFile};
use reqwest::header::{ACCEPT, CONTENT_TYPE, COOKIE, HeaderMap, HeaderValue, SET_COOKIE};
use reqwest::{Client, StatusCode};
use std::sync::Mutex;
use std::time::Duration;

/// 用户注册表在 SB 中的路径（与 V1 相同，内容 schema 升级为 v2）。
pub const USERS_PATH: &str = "web/opencode/config.md";

#[derive(Debug)]
pub struct SbClient {
    base_url: String,
    cookie_name: String,
    username: String,
    password: String,
    cookie: Mutex<Option<String>>,
    http: Client,
}

impl SbClient {
    pub fn new(
        base_url: impl Into<String>,
        username: impl Into<String>,
        password: impl Into<String>,
    ) -> AppResult<Self> {
        let base_url = base_url.into().trim_end_matches('/').to_string();
        let cookie_name = derive_cookie_name(&base_url);
        let http = Client::builder()
            .timeout(Duration::from_secs(10))
            .build()
            .map_err(|e| AppError::Internal(format!("reqwest build: {e}")))?;
        Ok(Self {
            base_url,
            cookie_name,
            username: username.into(),
            password: password.into(),
            cookie: Mutex::new(None),
            http,
        })
    }

    pub fn cookie(&self) -> Option<String> {
        self.cookie.lock().unwrap().clone()
    }

    /// `POST /.auth` form 登录；幂等（先清旧 cookie）。
    pub async fn login(&self) -> AppResult<()> {
        *self.cookie.lock().unwrap() = None;
        let login_url = format!("{}/.auth", self.base_url);
        let resp = self
            .http
            .post(&login_url)
            .header(ACCEPT, "*/*")
            .form(&[("username", self.username.as_str()), ("password", self.password.as_str())])
            .send()
            .await
            .map_err(|e| AppError::ServiceUnavailable(format!("sb login: {e}")))?;
        let status = resp.status();
        if !status.is_success() {
            return Err(AppError::ServiceUnavailable(format!(
                "sb login status: {status} (check SB_USER/SB_PASSWORD)"
            )));
        }
        let cookie = extract_cookie(resp.headers(), &self.cookie_name).ok_or_else(|| {
            AppError::ServiceUnavailable(format!(
                "sb login response missing cookie '{}'",
                self.cookie_name
            ))
        })?;
        *self.cookie.lock().unwrap() = Some(cookie);
        Ok(())
    }

    /// `GET /.fs/<path>`，401 自动重登一次。
    pub async fn get_fs(&self, path: &str) -> AppResult<String> {
        let url = format!("{}/.fs/{}", self.base_url, path.trim_start_matches('/'));
        let first = self.send_get(&url).await?;
        if first.status() == StatusCode::UNAUTHORIZED {
            self.login().await?;
            let second = self.send_get(&url).await?;
            return Self::read_text(second, path).await;
        }
        Self::read_text(first, path).await
    }

    /// `PUT /.fs/<path>`，401 自动重登一次。
    pub async fn put_fs(&self, path: &str, body: &str) -> AppResult<()> {
        let url = format!("{}/.fs/{}", self.base_url, path.trim_start_matches('/'));
        let first = self.send_put(&url, body).await?;
        if first.status() == StatusCode::UNAUTHORIZED {
            self.login().await?;
            let second = self.send_put(&url, body).await?;
            return Self::expect_ok(second, "sb put_fs");
        }
        Self::expect_ok(first, "sb put_fs")
    }

    /// 读取用户注册表：404→空表；V2 JSON 直读；V1 格式自动迁移并写回。
    pub async fn get_users(&self) -> AppResult<UsersFile> {
        let raw = match self.get_fs(USERS_PATH).await {
            Ok(raw) => raw,
            Err(AppError::NotFound(_)) => return Ok(UsersFile { version: 2, users: vec![] }),
            Err(e) => return Err(e),
        };
        // 先按 V2 解析（camelCase）
        if let Ok(f) = serde_json::from_str::<UsersFile>(&raw) {
            if f.version == 2 {
                return Ok(f);
            }
        }
        // V1（snake_case）→ 迁移并 best-effort 写回
        let migrated = migrate_v1(&raw)
            .map_err(|e| AppError::Internal(format!("用户注册表格式无法识别: {e}")))?;
        if let Err(e) = self.put_users(&migrated).await {
            tracing::warn!(%e, "V1 注册表迁移后写回失败（不阻塞读取）");
        }
        Ok(migrated)
    }

    /// 整体写回用户注册表。
    pub async fn put_users(&self, file: &UsersFile) -> AppResult<()> {
        let body = serde_json::to_string_pretty(file)
            .map_err(|e| AppError::Internal(format!("注册表序列化失败: {e}")))?;
        self.put_fs(USERS_PATH, &body).await
    }

    async fn send_get(&self, url: &str) -> AppResult<reqwest::Response> {
        let mut req = self.http.get(url).header(ACCEPT, "*/*");
        if let Some(c) = self.cookie() {
            if let Ok(v) = HeaderValue::from_str(&c) {
                req = req.header(COOKIE, v);
            }
        }
        req.send()
            .await
            .map_err(|e| AppError::ServiceUnavailable(format!("sb get_fs: {e}")))
    }

    async fn send_put(&self, url: &str, body: &str) -> AppResult<reqwest::Response> {
        let mut req = self
            .http
            .put(url)
            .header(ACCEPT, "*/*")
            .header(CONTENT_TYPE, "text/markdown; charset=utf-8")
            .body(body.to_string());
        if let Some(c) = self.cookie() {
            if let Ok(v) = HeaderValue::from_str(&c) {
                req = req.header(COOKIE, v);
            }
        }
        req.send()
            .await
            .map_err(|e| AppError::ServiceUnavailable(format!("sb put_fs: {e}")))
    }

    async fn read_text(resp: reqwest::Response, path: &str) -> AppResult<String> {
        let status = resp.status();
        if status == StatusCode::NOT_FOUND {
            return Err(AppError::NotFound(path.to_string()));
        }
        if !status.is_success() {
            return Err(AppError::ServiceUnavailable(format!(
                "sb get_fs status: {status}"
            )));
        }
        resp.text()
            .await
            .map_err(|e| AppError::ServiceUnavailable(format!("sb get_fs body: {e}")))
    }

    fn expect_ok(resp: reqwest::Response, ctx: &str) -> AppResult<()> {
        let status = resp.status();
        if status.is_success() {
            Ok(())
        } else {
            Err(AppError::ServiceUnavailable(format!("{ctx} status: {status}")))
        }
    }
}

/// `https://sb.example.com` → `auth_sb_example_com`。
fn derive_cookie_name(base_url: &str) -> String {
    let after_scheme = base_url.split_once("://").map_or(base_url, |(_, r)| r);
    let host_and_port = after_scheme.split('/').next().unwrap_or(after_scheme);
    let host = host_and_port.split(':').next().unwrap_or(host_and_port);
    format!("auth_{}", host.replace('.', "_"))
}

fn extract_cookie(headers: &HeaderMap, cookie_name: &str) -> Option<String> {
    let prefix = format!("{cookie_name}=");
    for value in headers.get_all(SET_COOKIE) {
        if let Ok(s) = value.to_str() {
            if let Some(rest) = s.strip_prefix(&prefix) {
                if let Some(cookie_part) = rest.split(';').next() {
                    return Some(format!("{prefix}{cookie_part}"));
                }
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    #[test]
    fn derive_cookie_name_basic() {
        assert_eq!(
            super::derive_cookie_name("https://sb.example.com"),
            "auth_sb_example_com"
        );
        assert_eq!(super::derive_cookie_name("http://127.0.0.1:3000"), "auth_127_0_0_1");
    }
}
