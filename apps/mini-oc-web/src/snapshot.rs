//! 设备数据快照：live 拉取成功后 best-effort 写入 SB，离线时回退读取。
//!
//! 路径：`serv/opencode/snapshots/{user_id}/{device_name}/{projects|sessions}.md`
//! 内容：`{fetched_at, data}` JSON（无凭据）。

use crate::error::AppResult;
use crate::sb::SbClient;
use mini_oc_core::ocapi::{Project, SessionInfo};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;

#[derive(Debug, Serialize, Deserialize)]
pub struct Snapshot<T> {
    #[serde(rename = "fetched_at")]
    pub fetched_at: String,
    pub data: Vec<T>,
}

pub fn snapshot_path(user_id: &str, device_name: &str, kind: &str) -> String {
    format!("serv/opencode/snapshots/{user_id}/{device_name}/{kind}.md")
}

/// live 结果内存缓存（60s 防抖，per user+device+kind+directory）。
#[derive(Debug)]
pub struct LiveCache {
    inner: Mutex<std::collections::HashMap<String, (std::time::Instant, serde_json::Value)>>,
    ttl: Duration,
}

impl LiveCache {
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(std::collections::HashMap::new()),
            ttl: Duration::from_secs(60),
        }
    }

    pub async fn get(&self, key: &str) -> Option<serde_json::Value> {
        let mut map = self.inner.lock().await;
        if let Some((at, v)) = map.get(key) {
            if at.elapsed() < self.ttl {
                return Some(v.clone());
            }
            map.remove(key);
        }
        None
    }

    pub async fn put(&self, key: &str, v: serde_json::Value) {
        self.inner
            .lock()
            .await
            .insert(key.to_string(), (std::time::Instant::now(), v));
    }
}

/// live 拉取成功后的快照异步写（不阻塞响应；失败仅告警）。
pub fn spawn_snapshot_write(sb: Arc<SbClient>, path: String, body: String) {
    tokio::spawn(async move {
        if let Err(e) = sb.put_fs(&path, &body).await {
            tracing::warn!(%e, path = %path, "快照写回失败（不影响本次响应）");
        }
    });
}

pub async fn read_projects_snapshot(sb: &SbClient, user_id: &str, device: &str) -> Option<Snapshot<Project>> {
    read_snapshot(sb, &snapshot_path(user_id, device, "projects")).await
}

pub async fn read_sessions_snapshot(sb: &SbClient, user_id: &str, device: &str) -> Option<Snapshot<SessionInfo>> {
    read_snapshot(sb, &snapshot_path(user_id, device, "sessions")).await
}

async fn read_snapshot<T: for<'de> Deserialize<'de>>(sb: &SbClient, path: &str) -> Option<Snapshot<T>> {
    match sb.get_fs(path).await {
        Ok(raw) => serde_json::from_str(&raw).ok(),
        Err(_) => None, // 404 或 SB 不可达：无快照
    }
}

/// 双源响应装配：live 成功 → source=live + 触发快照写；失败 → source=snapshot + 快照数据。
///
/// 禁止 live 失败时静默当成功：响应必须带 source 标注。
pub async fn dual_source<T: Serialize + serde::de::DeserializeOwned + Clone>(
    sb: Arc<SbClient>,
    user_id: &str,
    device: &str,
    kind: &str,
    live: Result<Vec<T>, crate::error::AppError>,
) -> AppResult<serde_json::Value> {
    match live {
        Ok(data) => {
            let snap = Snapshot {
                fetched_at: chrono::Utc::now().to_rfc3339(),
                data: data.clone(),
            };
            if let Ok(body) = serde_json::to_string(&snap) {
                spawn_snapshot_write(sb.clone(), snapshot_path(user_id, device, kind), body);
            }
            Ok(serde_json::json!({"source": "live", "fetchedAt": null, "data": data}))
        }
        Err(_) => {
            let raw = sb.get_fs(&snapshot_path(user_id, device, kind)).await.ok();
            let parsed = raw.and_then(|r| serde_json::from_str::<Snapshot<T>>(&r).ok());
            match parsed {
                Some(snap) => Ok(serde_json::json!({
                    "source": "snapshot", "fetchedAt": snap.fetched_at, "data": snap.data
                })),
                None => Ok(serde_json::json!({
                    "source": "snapshot", "fetchedAt": null, "data": []
                })),
            }
        }
    }
}
