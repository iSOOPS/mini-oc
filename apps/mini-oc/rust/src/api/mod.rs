//! frb 入口模块：暴露给 Dart 的函数与类型。
//!
//! 只有本命名空间内的 pub 项生成 Dart 绑定（flutter_rust_bridge.yaml 限定 crate::api）。

pub use crate::account::{bind_device, login, logout, AccountInfo, DeviceBrief};
pub use crate::config::{load_config, save_config, ClientConfig, StoredUser};
// frb 需要类型在 api 命名空间可见
pub use crate::logbuf::LogLine;
pub use crate::supervisor::ServiceState;

use crate::supervisor::{SupConfig, Supervisor};
use mini_oc_core::ocapi::{OcApi, OcClient};
use serde::{Deserialize, Serialize};
use std::sync::OnceLock;

/// 桥接链路自检（同步）。
#[flutter_rust_bridge::frb(sync)]
pub fn ping() -> String {
    "pong".into()
}

pub mod simple;

// ---------------------------------------------------------------------------
// 服务编排（全局单例）
// ---------------------------------------------------------------------------

/// 真实 opencode 命令构造：OPENCODE_SERVER_PASSWORD=<user.key> env 注入（不进命令行）。
fn real_serve_command() -> Box<dyn Fn(u16) -> std::process::Command + Send + Sync> {
    Box::new(|port: u16| {
        let mut cmd = std::process::Command::new(resolve_opencode_bin());
        cmd.arg("serve").arg("--port").arg(port.to_string());
        if let Some(user) = crate::config::load_config().user {
            cmd.env("OPENCODE_SERVER_PASSWORD", user.key);
        }
        cmd
    })
}

/// opencode 二进制定位：OPENCODE_BIN env → config.opencode_bin → PATH。
pub fn resolve_opencode_bin() -> String {
    if let Ok(p) = std::env::var("OPENCODE_BIN") {
        if !p.is_empty() {
            return p;
        }
    }
    if let Some(p) = crate::config::load_config().opencode_bin {
        if !p.is_empty() {
            return p;
        }
    }
    "opencode".into()
}

/// 真实探针：GET /api/info（basic opencode:<key>）。
fn real_probe() -> Box<dyn Fn(u16) -> crate::supervisor::ProbeFuture + Send + Sync> {
    Box::new(|port: u16| {
        let key = crate::config::load_config()
            .user
            .map(|u| u.key)
            .unwrap_or_default();
        Box::pin(async move {
            let client = OcClient::new(&format!("http://127.0.0.1:{port}"), &key);
            client
                .info()
                .await
                .map(|i| i.version)
                .map_err(|e| e.to_string())
        })
    })
}

/// 真实 rathole 命令：rathole <config.toml>。
fn real_rathole_command() -> Box<dyn Fn(&std::path::Path) -> std::process::Command + Send + Sync> {
    Box::new(|toml_path: &std::path::Path| {
        let bin = crate::rathole::resolve_rathole_bin()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|| "rathole".into());
        let mut cmd = std::process::Command::new(bin);
        cmd.arg(toml_path);
        cmd
    })
}

fn global_supervisor() -> &'static Supervisor {
    static SUP: OnceLock<Supervisor> = OnceLock::new();
    SUP.get_or_init(|| {
        let sup = Supervisor::new(SupConfig {
            serve_command: real_serve_command(),
            rathole_command: real_rathole_command(),
            probe: real_probe(),
        });
        // 后台监督 + 日志节流循环
        tokio::spawn(async move {
            // run_monitor 需要持有同一实例——通过再次 get（OnceLock 已初始化）
            if let Some(s) = SUP.get() {
                s.run_monitor().await;
            }
        });
        tokio::spawn(async move {
            crate::logbuf::flush_loop().await;
        });
        sup
    })
}

/// 当前应使用的端口：绑定设备（云服务场景以清单为准）→ 本地偏好。
fn effective_port() -> Result<u16, String> {
    let cfg = crate::config::load_config();
    Ok(cfg.port)
}

/// 绑定设备的清单条目（从服务器刷新后返回）。
async fn bound_device_brief() -> Result<Option<DeviceBrief>, String> {
    let cfg = crate::config::load_config();
    let Some(user) = cfg.user else {
        return Err("未登录".into());
    };
    let Some(bound) = cfg.bound_device else {
        return Ok(None);
    };
    let info = login(&cfg.server_url, &user.key).await?;
    Ok(info.devices.into_iter().find(|d| d.name == bound))
}

/// 启动本地服务。
pub async fn start_local() -> Result<(), String> {
    crate::logbuf::push_log("system", "info", "启动本地服务…");
    let port = effective_port()?;
    global_supervisor().start_local(port).await
}

/// 启动云服务（需已登录并绑定设备；端口以清单为准）。
pub async fn start_cloud() -> Result<(), String> {
    let Some(device) = bound_device_brief().await? else {
        return Err("尚未绑定设备：请先在登录后选择要绑定的设备".into());
    };
    crate::logbuf::push_log("system", "info", &format!("启动云服务（设备 {}，端口 {}）…", device.name, device.port));
    // rathole toml 生成（设备清单 → 单 service）
    let cfg = crate::config::load_config();
    let user = cfg.user.ok_or("未登录")?;
    let toml = crate::rathole::rathole_toml(&device, &user.key)?;
    let toml_dir = rathole_settings_dir()?;
    std::fs::create_dir_all(&toml_dir).map_err(|e| format!("创建 rathole 目录失败: {e}"))?;
    let toml_path = toml_dir.join("global.toml");
    std::fs::write(&toml_path, &toml).map_err(|e| format!("写入 rathole 配置失败: {e}"))?;
    global_supervisor().start_cloud(device.port, &toml_path).await
}

fn rathole_settings_dir() -> Result<std::path::PathBuf, String> {
    Ok(crate::config::config_path().parent().map(|p| p.join("rathole/settings")).unwrap_or_else(|| "rathole/settings".into()))
}

/// 停止全部服务。
pub async fn stop_all() -> Result<(), String> {
    global_supervisor().stop_all().await
}

/// 服务状态快照。
pub fn service_state() -> ServiceState {
    global_supervisor().state()
}

/// 项目摘要（frb）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectSummary {
    pub id: String,
    pub name: String,
    pub path: String,
    pub vcs: Option<String>,
}

/// 会话摘要（frb）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionSummary {
    pub id: String,
    pub title: Option<String>,
    pub directory: String,
    pub updated_ms: i64,
}

/// 本地 serve 是否在运行（查询前置检查）。
fn require_serve() -> Result<u16, String> {
    let st = global_supervisor().state();
    if st.mode == crate::supervisor::ServiceMode::Off {
        return Err("服务未启动：请先启动本地或云服务".into());
    }
    Ok(st.port)
}

/// 内部：按端口查询项目（可测试注入）。
pub async fn list_projects_at(port: u16, key: &str) -> Result<Vec<ProjectSummary>, String> {
    let client = OcClient::new(&format!("http://127.0.0.1:{port}"), key);
    let projects = client.projects().await.map_err(|e| e.to_string())?;
    Ok(projects
        .into_iter()
        .map(|p| ProjectSummary {
            name: p
                .name
                .clone()
                .unwrap_or_else(|| p.canonical.rsplit('/').next().unwrap_or(&p.canonical).to_string()),
            id: p.id,
            path: p.canonical,
            vcs: p.vcs,
        })
        .collect())
}

/// 项目列表（连本地 serve）。
pub async fn list_projects() -> Result<Vec<ProjectSummary>, String> {
    let port = require_serve()?;
    let key = current_key()?;
    list_projects_at(port, &key).await
}

/// 内部：按端口查询会话。
pub async fn list_sessions_at(port: u16, key: &str, directory: Option<String>) -> Result<Vec<SessionSummary>, String> {
    let client = OcClient::new(&format!("http://127.0.0.1:{port}"), key);
    let sessions = client
        .sessions(directory.as_deref())
        .await
        .map_err(|e| e.to_string())?;
    Ok(sessions
        .into_iter()
        .map(|s| SessionSummary {
            id: s.id,
            title: s.title,
            directory: s.directory,
            updated_ms: s.time.updated,
        })
        .collect())
}

/// 会话列表（连本地 serve，可按目录过滤）。
pub async fn list_sessions(directory: Option<String>) -> Result<Vec<SessionSummary>, String> {
    let port = require_serve()?;
    let key = current_key()?;
    list_sessions_at(port, &key, directory).await
}

fn current_key() -> Result<String, String> {
    crate::config::load_config()
        .user
        .map(|u| u.key)
        .ok_or_else(|| "未登录".into())
}

/// 在系统终端新窗口打开（凭据零泄露：见 term_spawn 模块文档）。
pub fn open_in_terminal(directory: String, session_id: Option<String>) -> Result<(), String> {
    let port = require_serve()?;
    let key = current_key()?;
    crate::logbuf::push_log(
        "system",
        "info",
        &format!("打开终端：{directory}（会话 {:?}）", session_id),
    );
    crate::term_spawn::open_in_terminal(
        &resolve_opencode_bin(),
        &format!("http://127.0.0.1:{port}"),
        &directory,
        session_id.as_deref(),
        &key,
    )
}

use crate::frb_generated::StreamSink;

/// 日志订阅（frb StreamSink 标准流模式）。
pub fn subscribe_logs(sink: StreamSink<crate::logbuf::LogLine>) -> Result<(), String> {
    let mut rx = crate::logbuf::subscribe_logs_channel();
    tokio::spawn(async move {
        while let Some(line) = rx.recv().await {
            if sink.add(line).is_err() {
                break; // Dart 侧关闭
            }
        }
    });
    Ok(())
}

/// 服务状态流订阅（UI 监听模式切换/崩溃）。
pub fn subscribe_state(sink: StreamSink<ServiceState>) -> Result<(), String> {
    let mut rx = global_supervisor().subscribe();
    tokio::spawn(async move {
        // 先推当前快照
        if sink.add(global_supervisor().state()).is_ok() {
            while let Some(st) = rx.recv().await {
                if sink.add(st).is_err() {
                    break;
                }
            }
        }
    });
    Ok(())
}
