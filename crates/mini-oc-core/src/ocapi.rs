//! opencode v2 HTTP API 客户端。
//!
//! 实测事实（spec §2）：
//! - 鉴权 Basic Auth，用户名硬编码 `opencode`；
//! - `/api/session` 返回 `{data, cursor}` 分页结构；
//! - `/api/pair` POST → `{code, expires_in}`。

use serde::Deserialize;
use std::time::Duration;

/// API 错误。`Unauthorized` 与 `Transport` 的区分是探测三态的判别依据。
#[derive(Debug, thiserror::Error)]
pub enum OcError {
    #[error("鉴权失败 (HTTP {0})：凭据与设备端不一致")]
    Unauthorized(u16),
    #[error("连接失败：设备不可达")]
    Transport,
    #[error("服务端响应异常: {0}")]
    Other(String),
}

// ---------------------------------------------------------------------------
// 响应类型（宽容反序列化：多余/缺省字段不炸）
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, serde::Serialize, Deserialize, PartialEq)]
pub struct ServerInfo {
    /// opencode 版本号（探测响应展示用，Task 6 使用）
    #[allow(dead_code)]
    pub version: String,
    pub pid: u64,
}

#[derive(Debug, Clone, serde::Serialize, Deserialize, PartialEq)]
pub struct Project {
    pub id: String,
    pub canonical: String,
    #[serde(default)]
    pub vcs: Option<String>,
    #[serde(default)]
    pub name: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize, Deserialize, PartialEq)]
pub struct SessionTime {
    pub created: i64,
    pub updated: i64,
}

/// 会话摘要（附A 结构的字段子集）。
/// 会话摘要（附A 结构的字段子集；Deserialize 为自定义宽松实现，见下方）。
#[derive(Debug, Clone, serde::Serialize, PartialEq)]
pub struct SessionInfo {
    pub id: String,
    #[serde(rename = "projectID", default)]
    pub project_id: String,
    #[serde(default)]
    pub title: Option<String>,
    /// location.directory（宽松反序列化兼容平级/嵌套两形态）
    #[serde(default)]
    pub directory: String,
    pub time: SessionTime,
}

impl SessionInfo {
    /// 从附A 原始 JSON 提取 directory 到平级字段。
    pub fn from_value(v: serde_json::Value) -> Result<Self, serde_json::Error> {
        serde_json::from_value(v)
    }
}

/// 宽松反序列化：兼容「location.directory 嵌套」（opencode API 原始形态，
/// 亦即快照存储形态）与「directory 平级」两种输入。
impl<'de> serde::Deserialize<'de> for SessionInfo {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        #[derive(serde::Deserialize)]
        struct Raw {
            id: String,
            #[serde(default)]
            project_id: Option<String>,
            #[serde(rename = "projectID", default)]
            project_id_upper: Option<String>,
            #[serde(default)]
            title: Option<String>,
            #[serde(default)]
            directory: Option<String>,
            #[serde(default)]
            location: Option<RawLocation>,
            time: SessionTime,
        }
        #[derive(serde::Deserialize)]
        struct RawLocation {
            #[serde(default)]
            directory: Option<String>,
        }
        let raw: Raw = Raw::deserialize(d)?;
        let directory = raw
            .directory
            .or_else(|| raw.location.and_then(|l| l.directory))
            .unwrap_or_default();
        let project_id = raw
            .project_id_upper
            .or(raw.project_id)
            .unwrap_or_default();
        Ok(SessionInfo {
            id: raw.id,
            project_id,
            title: raw.title,
            directory,
            time: raw.time,
        })
    }
}

#[derive(Debug, Deserialize)]
struct SessionsPage {
    #[serde(default)]
    data: Vec<serde_json::Value>,
    #[serde(default)]
    cursor: Option<SessionsCursor>,
}

#[derive(Debug, Deserialize)]
struct SessionsCursor {
    #[serde(default)]
    next: Option<String>,
}

// ---------------------------------------------------------------------------
// trait（web 层 wiremock 注入用）
// ---------------------------------------------------------------------------

#[async_trait::async_trait]
pub trait OcApi: Send + Sync {
    async fn info(&self) -> Result<ServerInfo, OcError>;
    async fn projects(&self) -> Result<Vec<Project>, OcError>;
    async fn sessions(&self, directory: Option<&str>) -> Result<Vec<SessionInfo>, OcError>;
    async fn pair(&self) -> Result<String, OcError>;
    async fn connect(&self, code: &str) -> Result<String, OcError>;
}

// ---------------------------------------------------------------------------
// 默认实现
// ---------------------------------------------------------------------------

/// 每页拉取上限。
const PAGE_LIMIT: u32 = 50;
/// 最多翻页数（防异常服务端死循环）。
const MAX_PAGES: usize = 10;
/// 单请求超时。
const REQUEST_TIMEOUT: Duration = Duration::from_secs(4);

pub struct OcClient {
    base: String,
    password: String,
    http: reqwest::Client,
}

impl OcClient {
    pub fn new(base_url: &str, password: &str) -> Self {
        let base = base_url.trim_end_matches('/').to_string();
        Self {
            base,
            password: password.to_string(),
            http: reqwest::Client::builder()
                .timeout(REQUEST_TIMEOUT)
                .build()
                .expect("reqwest client 构建失败"),
        }
    }

    async fn get_json<T: serde::de::DeserializeOwned>(&self, path: &str, query: &[(&str, String)]) -> Result<T, OcError> {
        let url = format!("{}{path}", self.base);
        let mut req = self.http.get(&url).basic_auth("opencode", Some(&self.password));
        if !query.is_empty() {
            req = req.query(query);
        }
        let resp = req.send().await.map_err(|_| OcError::Transport)?;
        Self::decode(resp).await
    }

    async fn decode<T: serde::de::DeserializeOwned>(resp: reqwest::Response) -> Result<T, OcError> {
        match resp.status() {
            s if s.is_success() => resp
                .json::<T>()
                .await
                .map_err(|e| OcError::Other(format!("响应解析失败: {e}"))),
            reqwest::StatusCode::UNAUTHORIZED => Err(OcError::Unauthorized(401)),
            reqwest::StatusCode::FORBIDDEN => Err(OcError::Unauthorized(403)),
            s => Err(OcError::Other(format!("HTTP {s}"))),
        }
    }
}

#[async_trait::async_trait]
impl OcApi for OcClient {
    async fn info(&self) -> Result<ServerInfo, OcError> {
        self.get_json("/api/info", &[]).await
    }

    async fn projects(&self) -> Result<Vec<Project>, OcError> {
        self.get_json("/api/project", &[]).await
    }

    async fn sessions(&self, directory: Option<&str>) -> Result<Vec<SessionInfo>, OcError> {
        let mut all: Vec<SessionInfo> = Vec::new();
        let mut cursor: Option<String> = None;
        for _ in 0..MAX_PAGES {
            let mut query: Vec<(String, String)> = vec![("limit".into(), PAGE_LIMIT.to_string())];
            if let Some(dir) = directory {
                query.push(("directory".into(), dir.to_string()));
            }
            if let Some(c) = &cursor {
                query.push(("cursor".into(), c.clone()));
            }
            let query_ref: Vec<(&str, String)> =
                query.iter().map(|(k, v)| (k.as_str(), v.clone())).collect();
            let page: SessionsPage = self.get_json("/api/session", &query_ref).await?;
            for item in page.data {
                match SessionInfo::from_value(item) {
                    Ok(s) => all.push(s),
                    Err(e) => tracing::warn!(%e, "跳过无法解析的会话条目"),
                }
            }
            cursor = page.cursor.and_then(|c| c.next);
            if cursor.is_none() {
                break;
            }
        }
        Ok(all)
    }

    async fn pair(&self) -> Result<String, OcError> {
        let url = format!("{}/api/pair", self.base);
        let resp = self
            .http
            .post(&url)
            .basic_auth("opencode", Some(&self.password))
            .send()
            .await
            .map_err(|_| OcError::Transport)?;
        #[derive(Deserialize)]
        struct PairResp {
            code: String,
        }
        let resp: PairResp = Self::decode(resp).await?;
        Ok(resp.code)
    }

    /// 用一次性 pair code 换 pair token（`GET /auth/connect/{code}` → `{token}`）。
    ///
    /// 端点公开（code 即凭据），无需 basic auth；curl 无 -u 实测返回 JSON，
    /// 而浏览器形态（Accept: text/html）才 302 → `/`。故此处用 reqwest 默认
    /// 无 Accept 头拿 JSON。code 一次性：复用/伪造返回 401。
    async fn connect(&self, code: &str) -> Result<String, OcError> {
        let url = format!("{}/auth/connect/{code}", self.base);
        let resp = self.http.get(&url).send().await.map_err(|_| OcError::Transport)?;
        #[derive(Deserialize)]
        struct ConnectResp {
            token: String,
        }
        let resp: ConnectResp = Self::decode(resp).await?;
        Ok(resp.token)
    }
}
