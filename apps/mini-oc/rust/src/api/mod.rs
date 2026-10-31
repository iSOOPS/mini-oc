//! frb 入口模块：暴露给 Dart 的函数与类型。
//!
//! 只有本命名空间内的 pub 项生成 Dart 绑定（flutter_rust_bridge.yaml 限定 crate::api）。
//! 注意：frb 2.11 不展开 pub use 的函数——包装函数直接定义于此。

pub use crate::account::DeviceBrief;
pub use crate::config::{ClientConfig, StoredUser};
pub use crate::logbuf::LogLine;
pub use crate::supervisor::ServiceState;

// appenv 类型透传给 Dart（frb 需要类型定义在命名空间内可达）
pub use crate::api::appenv::AppEnv;

use crate::supervisor::{SupConfig, Supervisor};
use mini_oc_core::ocapi::{OcApi, OcClient};
use serde::{Deserialize, Serialize};
use std::sync::OnceLock;

/// 登录（见 account::login）。
pub async fn login(server_url: String, key: String) -> Result<crate::account::AccountInfo, String> {
    crate::account::login(&server_url, &key).await
}

/// 绑定设备（见 account::bind_device）。
pub async fn bind_device(device_name: String, pctype: String, os: String) -> Result<(), String> {
    crate::account::bind_device(&device_name, &pctype, &os).await
}

/// 登出（见 account::logout）。
pub fn logout() -> Result<(), String> {
    crate::account::logout()
}

/// 读应用 env 配置（地址 / 记住密码 / 密码；同步：本地文件操作）。
#[flutter_rust_bridge::frb(sync)]
pub fn load_app_env() -> crate::api::appenv::AppEnv {
    crate::api::appenv::load_app_env()
}

/// 写应用 env 配置（记住密码逻辑；docker 注入时不落文件）。
pub fn save_app_env(cfg: crate::api::appenv::AppEnv) -> Result<(), String> {
    crate::api::appenv::save_app_env(&cfg)
}

/// 生效服务器地址：env → config.json 兜底 → 默认 https://oc.isoops.com。
#[flutter_rust_bridge::frb(sync)]
pub fn server_url_effective() -> String {
    crate::api::appenv::server_url_effective()
}

/// 读本地配置（同步：本地文件操作）。
#[flutter_rust_bridge::frb(sync)]
pub fn load_config() -> ClientConfig {
    crate::config::load_config()
}

/// 写本地配置。
pub fn save_config(cfg: ClientConfig) -> Result<(), String> {
    crate::config::save_config(&cfg)
}

/// 桥接链路自检（同步）。
#[flutter_rust_bridge::frb(sync)]
pub fn ping() -> String {
    "pong".into()
}

pub mod appenv;
pub mod simple;

// ---------------------------------------------------------------------------
// 端口占用处理（I10）
// ---------------------------------------------------------------------------

/// 端口占用错误前缀（Dart 侧判定弹框用；与 Rust 常量同值，frb 不透传 const）。
#[flutter_rust_bridge::frb(sync)]
pub fn port_err_prefix() -> String {
    crate::portkill::PORT_ERR_PREFIX.to_string()
}

/// 查询端口占用进程（无占用返回 null）。
#[flutter_rust_bridge::frb(sync)]
pub fn query_port_occupant(port: u16) -> Option<crate::portkill::PortOccupant> {
    crate::portkill::port_occupant(port)
}

/// 杀掉端口占用者并等待端口释放（详见 portkill 模块文档）。
pub async fn kill_port_occupant(port: u16) -> Result<(), String> {
    // lsof/ps/杀进程 + 端口轮询均为阻塞调用，放 blocking 线程池防卡 frb runtime
    tokio::task::spawn_blocking(move || crate::portkill::kill_port(port))
        .await
        .map_err(|e| format!("kill_port 任务失败: {e}"))?
}

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
        // （I9：跑共享 runtime——本函数可能在无 tokio 上下文的线程被调起）
        crate::runtime::spawn_on_runtime(async move {
            // run_monitor 需要持有同一实例——通过再次 get（OnceLock 已初始化）
            if let Some(s) = SUP.get() {
                s.run_monitor().await;
            }
        });
        crate::runtime::spawn_on_runtime(async move {
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
    let info = crate::account::login(&cfg.server_url, &user.key).await?;
    // 自动登录冷启动挂点：清单拉取成功 → 静默上报一次（幂等，服务端重复无害）
    crate::runtime::spawn_on_runtime(async move {
        let out = crate::api::report_registration().await;
        if !out.ok {
            tracing::warn!("设备上报未成功（不影响云服务）: {:?}", out.error);
        }
    });
    Ok(info.devices.into_iter().find(|d| d.name == bound))
}

// ---------------------------------------------------------------------------
// 设备上报注册
// ---------------------------------------------------------------------------

/// 上报结果（永不 panic：失败时 ok=false，错误装进 error 字段）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReportOutcome {
    pub ok: bool,
    /// approved | pending；失败为空串
    pub status: String,
    /// 服务端确认的设备 ID；失败为空串
    pub device_id: String,
    pub enabled: bool,
    /// 批准设备是否已自动完成绑定（approved + enabled + 绑定请求成功）
    pub bound: bool,
    pub error: Option<String>,
}

/// 上报端口：绑定设备场景以清单端口为准不可得 → 退本地偏好（与 effective_port 一致）。
fn report_port() -> u16 {
    effective_port().unwrap_or(crate::config::ClientConfig::default().port)
}

/// 上报本机设备信息到 /api/device-register（幂等，重复调用无害）。
///
/// - 地址取 `server_url_effective()`（env 优先，比读 config 更稳）
/// - 响应 deviceId 与 config 不同 → 原子写回 config.json
/// - approved + enabled → 自动绑定：拉清单找到本机设备 → device-bind →
///   本地 bound_device 写盘（管理员批准即信任，免去重新登录选择）
/// - 永不 panic：所有错误装进 `ReportOutcome.error`，调用方静默降级
pub async fn report_registration() -> ReportOutcome {
    let inner = async {
        let cfg = crate::config::load_config();
        let user = cfg.user.as_ref().ok_or("未登录：无法上报设备")?;
        let server = server_url_effective();
        let os = std::env::consts::OS.to_string();
        let body = crate::account::RegisterBody {
            key: user.key.clone(),
            device_id: cfg.device_id.clone().unwrap_or_default(),
            desc: String::new(),
            port: report_port(),
            pctype: os.clone(),
            device_name: whoami::devicename(),
            os,
        };
        crate::account::register_device(&server, &body).await
    };
    match inner.await {
        Ok(result) => {
            // 响应 deviceId 与 config 不同 → 写回（save_config 本身是原子替换；空 deviceId 视为异常响应不写）
            let mut cfg = crate::config::load_config();
            if !result.device_id.is_empty()
                && cfg.device_id.as_deref() != Some(result.device_id.as_str())
            {
                cfg.device_id = Some(result.device_id.clone());
                if let Err(e) = crate::config::save_config(&cfg) {
                    tracing::warn!("device_id 写回配置失败: {e}");
                }
            }
            // 批准 + 启用 → 自动绑定（幂等：已绑定设备重复 bind 无害）
            let bound = if result.status == "approved" && result.enabled {
                auto_bind_approved_device(&result.device_id).await.unwrap_or_else(|e| {
                    tracing::warn!("批准后自动绑定未成功（不影响上报）: {e}");
                    false
                })
            } else {
                false
            };
            ReportOutcome {
                ok: true,
                status: result.status,
                device_id: result.device_id,
                enabled: result.enabled,
                bound,
                error: None,
            }
        }
        Err(e) => {
            tracing::warn!("设备上报失败（静默降级）: {e}");
            ReportOutcome {
                ok: false,
                status: String::new(),
                device_id: String::new(),
                enabled: false,
                bound: false,
                error: Some(e),
            }
        }
    }
}

/// 只读上报状态查询（I12：设置页进入时调用，无副作用）。
///
/// 与 [`report_registration`] 的区别：不 POST device-register、不写 config、
/// 不触发自动绑定——仅拉清单判断本机设备当前状态，供 UI 恢复真实上报态
/// （App 重启后内存态丢失，不能显示「尚未上报」）。
///
/// 匹配顺序：config.device_id → 本机 device_name+pctype。
/// 清单未命中 → status="pending"（服务端不下发 pending 明细，无法区分
/// 「待审核」与「未上报」）；网络失败 → ok=false。
pub async fn query_registration() -> ReportOutcome {
    let inner = async {
        let cfg = crate::config::load_config();
        let user = cfg.user.clone().ok_or("未登录")?;
        let server = server_url_effective();
        let devices = fetch_devices(&server, &user.key).await?;
        let host = whoami::devicename();
        let os = std::env::consts::OS.to_string();
        let hit = devices.iter().find(|d| {
            !cfg.device_id.as_deref().unwrap_or("").is_empty()
                && d.device_id == cfg.device_id.as_deref().unwrap_or("")
        }).or_else(|| {
            devices.iter().find(|d| d.device_name == host && d.pctype == os)
        });
        Ok::<_, String>((hit.cloned(), cfg.device_id.clone().unwrap_or_default()))
    };
    match inner.await {
        Ok((Some(dev), known_id)) => ReportOutcome {
            ok: true,
            status: "approved".into(),
            device_id: if dev.device_id.is_empty() {
                known_id
            } else {
                dev.device_id.clone()
            },
            enabled: dev.enabled,
            bound: dev.bound,
            error: None,
        },
        Ok((None, _)) => ReportOutcome {
            ok: true,
            status: "pending".into(),
            device_id: String::new(),
            enabled: false,
            bound: false,
            error: None,
        },
        Err(e) => {
            tracing::warn!("上报状态查询失败: {e}");
            ReportOutcome {
                ok: false,
                status: String::new(),
                device_id: String::new(),
                enabled: false,
                bound: false,
                error: Some(e),
            }
        }
    }
}

/// 拉属主设备清单（GET-语义的 user/info，无副作用：不写配置、不触发上报）。
pub(crate) async fn fetch_devices(server_url: &str, key: &str) -> Result<Vec<crate::account::DeviceBrief>, String> {
    let resp = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .map_err(|e| format!("http client: {e}"))?
        .post(format!("{}/api/user/info", server_url.trim_end_matches('/')))
        .json(&serde_json::json!({"key": key}))
        .send()
        .await
        .map_err(|e| format!("网络错误：无法连接服务器（{e}）"))?;
    if !resp.status().is_success() {
        let status = resp.status();
        return Err(format!("拉取清单失败（HTTP {status}）"));
    }
    let info: crate::account::AccountInfo = resp
        .json()
        .await
        .map_err(|e| format!("响应解析失败: {e}"))?;
    Ok(info.devices)
}

/// 批准后自动绑定：拉属主清单 → 找到本机（device_name+pctype 匹配）→
/// device-bind 写服务端 bound=true + 本地 bound_device 持久化。
///
/// 失败返回 Err（调用方仅告警，不影响上报结果）。
async fn auto_bind_approved_device(_device_id: &str) -> Result<bool, String> {
    let cfg = crate::config::load_config();
    // 已绑定 → 无需重复（幂等出口）
    if cfg.bound_device.is_some() {
        return Ok(true);
    }
    let user = cfg.user.clone().ok_or("未登录")?;
    // 拉最新清单（无副作用版本）
    let devices = fetch_devices(&cfg.server_url, &user.key).await?;
    // DeviceBrief 无 device_id 字段 → 按 device_name（whoami 主机名）+ pctype 匹配
    let host = whoami::devicename();
    let pctype = std::env::consts::OS.to_string();
    let device = devices
        .into_iter()
        .find(|d| d.device_name == host && d.pctype == pctype)
        .ok_or("清单中未找到本机设备（可能尚未批准或已停用）")?;
    // 服务端核验 + 双端持久化
    let os = pctype.clone();
    crate::account::bind_device(&device.name, &pctype, &os).await?;
    tracing::info!("批准设备已自动绑定：{}", device.name);
    Ok(true)
}

/// 启动本地服务。
pub async fn start_local() -> Result<(), String> {
    crate::logbuf::push_log("system", "info", "启动本地服务…");
    let port = effective_port()?;
    global_supervisor().start_local(port).await
}

/// 停止本地服务（保留隧道）：LocalOnly → Off；Cloud → TunnelOnly（I20）。
pub async fn stop_local() -> Result<(), String> {
    crate::logbuf::push_log("system", "info", "停止本地服务…");
    global_supervisor().stop_serve().await
}

/// 生成 rathole toml（绑定设备清单 → 单 service）。
/// 返回 (设备端口, toml 路径)；隧道/全部启动共用。
async fn prepare_rathole() -> Result<(u16, std::path::PathBuf), String> {
    let Some(device) = bound_device_brief().await? else {
        return Err("尚未绑定设备：请先在登录后选择要绑定的设备".into());
    };
    // 前置检查（I17）：rathole 二进制不存在 → 可行动的明确错误
    // （否则 spawn ENOENT 只报泛化的「rathole 启动失败: No such file」）
    if crate::rathole::resolve_rathole_bin().is_none() {
        return Err(
            "未找到 rathole 客户端：请通过 RATHOLE_BIN 指定路径，或将 rathole 安装到 PATH".into(),
        );
    }
    let cfg = crate::config::load_config();
    let user = cfg.user.ok_or("未登录")?;
    let toml = crate::rathole::rathole_toml(&device, &user.key)?;
    let toml_dir = rathole_settings_dir()?;
    std::fs::create_dir_all(&toml_dir).map_err(|e| format!("创建 rathole 目录失败: {e}"))?;
    let toml_path = toml_dir.join("global.toml");
    crate::rathole::write_settings_toml(&toml_path, &toml)?;
    Ok((device.port, toml_path))
}

/// 仅启动隧道（I20）：只起 rathole，不启动 serve（需已登录并绑定设备）。
pub async fn start_tunnel() -> Result<(), String> {
    crate::logbuf::push_log("system", "info", "启动隧道（仅 rathole）…");
    let (_port, toml_path) = prepare_rathole().await?;
    global_supervisor().start_tunnel(&toml_path).await
}

/// 停止隧道（保留本地服务）：Cloud → LocalOnly；TunnelOnly → Off（I20）。
pub async fn stop_tunnel() -> Result<(), String> {
    crate::logbuf::push_log("system", "info", "停止隧道…");
    global_supervisor().stop_tunnel().await
}

/// 启动全部（I20，原「云服务」语义）：隧道 + 本地服务（端口以清单为准，
/// 同端口 serve 复用；已跑隧道复用）。
pub async fn start_all() -> Result<(), String> {
    let (port, toml_path) = prepare_rathole().await?;
    crate::logbuf::push_log(
        "system",
        "info",
        &format!("启动全部（本地服务 + 隧道，端口 {port}）…"),
    );
    global_supervisor().start_all(port, &toml_path).await
}

fn rathole_settings_dir() -> Result<std::path::PathBuf, String> {
    Ok(crate::config::config_path()
        .parent()
        .map(|p| p.join("rathole/settings"))
        .unwrap_or_else(|| "rathole/settings".into()))
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
            name: p.name.clone().unwrap_or_else(|| {
                p.canonical
                    .rsplit('/')
                    .next()
                    .unwrap_or(&p.canonical)
                    .to_string()
            }),
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
pub async fn list_sessions_at(
    port: u16,
    key: &str,
    directory: Option<String>,
) -> Result<Vec<SessionSummary>, String> {
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
///
/// 本函数以 wrap_normal 模式在 frb 线程池的普通线程上执行（无 tokio 上下文），
/// 桥接循环必须走共享 runtime（I9）。
pub fn subscribe_logs(sink: StreamSink<crate::logbuf::LogLine>) -> Result<(), String> {
    let mut rx = crate::logbuf::subscribe_logs_channel();
    crate::runtime::spawn_on_runtime(async move {
        while let Some(line) = rx.recv().await {
            if sink.add(line).is_err() {
                break; // Dart 侧关闭
            }
        }
    });
    Ok(())
}

/// 服务状态流订阅（UI 监听模式切换/崩溃）。
///
/// 同上：wrap_normal 模式 + 共享 runtime（I9）。
pub fn subscribe_state(sink: StreamSink<ServiceState>) -> Result<(), String> {
    let mut rx = global_supervisor().subscribe();
    crate::runtime::spawn_on_runtime(async move {
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
