//! 进程监督：opencode serve + rathole 编排状态机。
//!
//! 状态机（spec §5.4）：
//! ```text
//! Idle ──启动本地──► LocalOnly(serve)
//! Idle/LocalOnly ──启动云──► Cloud(serve+rathole)
//! Cloud ──降级──► LocalOnly（停 rathole 保 serve）
//! 任意 ──停止/崩溃──► Off（SIGTERM→5s→SIGKILL / taskkill /T /F）
//! ```
//!
//! 可测试性：命令构造与存活探针均由 `SupConfig` 注入（测试用 sleep 替身）。

use serde::Serialize;
use std::path::Path;
use std::process::Command;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::mpsc;

/// 存活探针返回 opencode 版本号。
pub type ProbeFuture = std::pin::Pin<Box<dyn std::future::Future<Output = Result<String, String>> + Send>>;

/// 进程命令构造器。
pub type ServeCommand = Box<dyn Fn(u16) -> Command + Send + Sync>;
pub type RatholeCommand = Box<dyn Fn(&Path) -> Command + Send + Sync>;
pub type Probe = Box<dyn Fn(u16) -> ProbeFuture + Send + Sync>;

/// 可注入配置（生产由 api 层构造真实命令；测试注入替身）。
pub struct SupConfig {
    pub serve_command: ServeCommand,
    pub rathole_command: RatholeCommand,
    pub probe: Probe,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq)]
pub enum ServiceMode {
    Off,
    LocalOnly,
    Cloud,
}

impl ServiceMode {
    pub fn mode_str(&self) -> &'static str {
        match self {
            ServiceMode::Off => "off",
            ServiceMode::LocalOnly => "local_only",
            ServiceMode::Cloud => "cloud",
        }
    }
}

/// frb 暴露的状态快照。
#[derive(Debug, Clone, Serialize)]
pub struct ServiceState {
    pub mode: ServiceMode,
    pub serve_pid: Option<u32>,
    pub rathole_pid: Option<u32>,
    pub port: u16,
    pub uptime_started: Option<i64>,
    pub last_error: Option<String>,
}

impl ServiceState {
    /// 模式字符串便捷访问（Dart 侧直接比较字符串）。
    pub fn mode_str(&self) -> &'static str {
        self.mode.mode_str()
    }
}

pub struct Supervisor {
    inner: Arc<Mutex<Inner>>,
    cmd: Arc<SupConfig>,
    tx: Arc<Mutex<mpsc::UnboundedSender<ServiceState>>>,
}

struct Inner {
    mode: ServiceMode,
    serve: Option<std::process::Child>,
    rathole: Option<std::process::Child>,
    port: u16,
    started_ms: Option<i64>,
    last_error: Option<String>,
}

impl Inner {
    fn snapshot(&self) -> ServiceState {
        ServiceState {
            mode: self.mode,
            serve_pid: self.serve.as_ref().map(|c| c.id()),
            rathole_pid: self.rathole.as_ref().map(|c| c.id()),
            port: self.port,
            uptime_started: self.started_ms,
            last_error: self.last_error.clone(),
        }
    }
}

impl Supervisor {
    pub fn new(cmd: SupConfig) -> Self {
        let (tx, _rx) = mpsc::unbounded_channel();
        Self {
            inner: Arc::new(Mutex::new(Inner {
                mode: ServiceMode::Off,
                serve: None,
                rathole: None,
                port: 0,
                started_ms: None,
                last_error: None,
            })),
            cmd: Arc::new(cmd),
            tx: Arc::new(Mutex::new(tx)),
        }
    }

    /// 订阅状态流（新订阅者接管旧通道；客户端 UI 单订阅点足够）。
    pub fn subscribe(&self) -> mpsc::UnboundedReceiver<ServiceState> {
        let (tx, rx) = mpsc::unbounded_channel();
        *self.tx.lock().unwrap() = tx;
        rx
    }

    pub fn state(&self) -> ServiceState {
        self.inner.lock().unwrap().snapshot()
    }

    fn emit(&self, st: ServiceState) {
        let _ = self.tx.lock().unwrap().send(st);
    }

    /// 端口占用检测。
    fn ensure_port_free(port: u16) -> Result<(), String> {
        match std::net::TcpListener::bind(("127.0.0.1", port)) {
            Ok(_) => Ok(()),
            Err(_) => Err(format!("端口 {port} 已被占用。可在设置中更换端口，或使用「强制清理」结束占用进程")),
        }
    }

    /// 启动本地服务：spawn serve（stdout/stderr 管道接入日志）→ 探针存活（10s）→ LocalOnly。
    pub async fn start_local(&self, port: u16) -> Result<(), String> {
        self.stop_all().await?;
        Self::ensure_port_free(port)?;
        let mut cmd = (self.cmd.serve_command)(port);
        #[cfg(target_os = "windows")]
        {
            use std::os::windows::process::CommandExt;
            cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
        }
        cmd.stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped());
        let mut child = cmd
            .spawn()
            .map_err(|e| format!("opencode 启动失败（检查二进制路径）: {e}"))?;
        spawn_traced(&mut child, "opencode");
        {
            let mut inner = self.inner.lock().unwrap();
            inner.serve = Some(child);
            inner.port = port;
        }
        // 存活探针（10s / 500ms 间隔）
        let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
        loop {
            let probe = (self.cmd.probe)(port);
            match probe.await {
                Ok(v) => {
                    let mut inner = self.inner.lock().unwrap();
                    inner.mode = ServiceMode::LocalOnly;
                    inner.started_ms = Some(chrono::Utc::now().timestamp_millis());
                    inner.last_error = None;
                    let st = inner.snapshot();
                    drop(inner);
                    self.emit(st);
                    tracing::info!(port, version = %v, "本地服务就绪");
                    return Ok(());
                }
                Err(e) => {
                    if tokio::time::Instant::now() >= deadline {
                        // 探针失败：回滚
                        self.stop_all().await?;
                        let msg = format!("opencode serve 未就绪：{e}");
                        {
                            let mut inner = self.inner.lock().unwrap();
                            inner.last_error = Some(msg.clone());
                        }
                        return Err(msg);
                    }
                    tokio::time::sleep(Duration::from_millis(500)).await;
                }
            }
        }
    }

    /// 启动云服务：serve（可复用）+ rathole → Cloud。
    pub async fn start_cloud(&self, port: u16, rathole_toml_path: &Path) -> Result<(), String> {
        // 端口不一致时以清单端口重启 serve（spec §5.2.1）
        let need_restart = {
            let inner = self.inner.lock().unwrap();
            inner.mode == ServiceMode::Off || inner.port != port
        };
        if need_restart {
            self.start_local(port).await?;
        }
        let mut cmd = (self.cmd.rathole_command)(rathole_toml_path);
        #[cfg(target_os = "windows")]
        {
            use std::os::windows::process::CommandExt;
            cmd.creation_flags(0x0800_0000);
        }
        cmd.stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped());
        let mut child = cmd
            .spawn()
            .map_err(|e| format!("rathole 启动失败: {e}"))?;
        spawn_traced(&mut child, "rathole");
        {
            let mut inner = self.inner.lock().unwrap();
            inner.rathole = Some(child);
            inner.mode = ServiceMode::Cloud;
            let st = inner.snapshot();
            drop(inner);
            self.emit(st);
        }
        // rathole 存活检查（1.5s）
        tokio::time::sleep(Duration::from_millis(1500)).await;
        let alive = {
            let mut inner = self.inner.lock().unwrap();
            let alive = inner
                .rathole
                .as_mut()
                .map(|c| c.try_wait().map(|s| s.is_none()).unwrap_or(false))
                .unwrap_or(false);
            if !alive {
                inner.rathole = None;
                inner.mode = ServiceMode::LocalOnly; // serve 仍在
                inner.last_error = Some("rathole 启动后立即退出（检查网络与 token）".into());
                false
            } else {
                true
            }
        };
        if !alive {
            return Err("rathole 启动失败：进程立即退出".into());
        }
        Ok(())
    }

    /// 降级：Cloud → LocalOnly。
    pub async fn downgrade_to_local(&self) -> Result<(), String> {
        self.kill_rathole().await?;
        let mut inner = self.inner.lock().unwrap();
        inner.mode = if inner.serve.is_some() { ServiceMode::LocalOnly } else { ServiceMode::Off };
        let st = inner.snapshot();
        drop(inner);
        self.emit(st);
        Ok(())
    }

    /// 停止全部。
    pub async fn stop_all(&self) -> Result<(), String> {
        self.kill_rathole().await?;
        self.kill_serve().await?;
        let mut inner = self.inner.lock().unwrap();
        inner.mode = ServiceMode::Off;
        inner.started_ms = None;
        let st = inner.snapshot();
        drop(inner);
        self.emit(st);
        Ok(())
    }

    async fn kill_serve(&self) -> Result<(), String> {
        let mut child = { self.inner.lock().unwrap().serve.take() };
        if let Some(c) = child.as_mut() {
            terminate_gracefully(c).await.map_err(|e| format!("停止 opencode 失败: {e}"))?;
        }
        Ok(())
    }

    async fn kill_rathole(&self) -> Result<(), String> {
        let mut child = { self.inner.lock().unwrap().rathole.take() };
        if let Some(c) = child.as_mut() {
            terminate_gracefully(c).await.map_err(|e| format!("停止 rathole 失败: {e}"))?;
        }
        Ok(())
    }

    /// 后台监督循环：进程崩溃 → Off + last_error（UI 可感知）。
    pub async fn run_monitor(&self) {
        loop {
            tokio::time::sleep(Duration::from_secs(1)).await;
            let mut inner = self.inner.lock().unwrap();
            let serve_alive = inner
                .serve
                .as_mut()
                .map(|c| c.try_wait().map(|s| s.is_none()).unwrap_or(false))
                .unwrap_or(false);
            let rathole_alive = inner
                .rathole
                .as_mut()
                .map(|c| c.try_wait().map(|s| s.is_none()).unwrap_or(false))
                .unwrap_or(false);
            match inner.mode {
                ServiceMode::Off => {}
                ServiceMode::LocalOnly if serve_alive => {}
                ServiceMode::Cloud if serve_alive && rathole_alive => {}
                ServiceMode::Cloud if serve_alive && !rathole_alive => {
                    inner.rathole = None;
                    inner.mode = ServiceMode::LocalOnly;
                    inner.last_error = Some("rathole 意外退出，已降级为本地服务".into());
                    let st = inner.snapshot();
                    drop(inner);
                    self.emit(st);
                    tracing::warn!("rathole 崩溃，降级本地");
                    continue;
                }
                _ => {
                    inner.serve = None;
                    inner.rathole = None;
                    inner.mode = ServiceMode::Off;
                    inner.started_ms = None;
                    inner.last_error = Some("opencode serve 意外退出".into());
                    let st = inner.snapshot();
                    drop(inner);
                    self.emit(st);
                    tracing::error!("serve 崩溃");
                }
            }
        }
    }
}

/// 子进程 stdout/stderr 逐行接入日志管道（I4：spec §5.6 日志数据源）。
/// std::process 管道是同步 IO——每管道一个阻塞线程，进程退出后管道关闭线程自然结束。
fn spawn_traced(child: &mut std::process::Child, source: &'static str) {
    if let Some(out) = child.stdout.take() {
        std::thread::spawn(move || {
            use std::io::BufRead;
            let reader = std::io::BufReader::new(out);
            for line in reader.lines().map_while(Result::ok) {
                crate::logbuf::push_log(source, "info", &line);
            }
        });
    }
    if let Some(err) = child.stderr.take() {
        std::thread::spawn(move || {
            use std::io::BufRead;
            let reader = std::io::BufReader::new(err);
            for line in reader.lines().map_while(Result::ok) {
                crate::logbuf::push_log(source, "warn", &line);
            }
        });
    }
}

/// 平台优雅终止：Unix SIGTERM→5s→SIGKILL；Windows taskkill /T /F（进程树）。
async fn terminate_gracefully(child: &mut std::process::Child) -> std::io::Result<()> {
    let pid = child.id();
    #[cfg(unix)]
    {
        use nix::sys::signal::{kill, Signal};
        use nix::unistd::Pid;
        let _ = kill(Pid::from_raw(pid as i32), Signal::SIGTERM);
        for _ in 0..50 {
            if child.try_wait()?.is_some() {
                return Ok(());
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        let _ = kill(Pid::from_raw(pid as i32), Signal::SIGKILL);
        let _ = child.wait();
        Ok(())
    }
    #[cfg(target_os = "windows")]
    {
        let out = tokio::task::spawn_blocking(move || {
            Command::new("taskkill")
                .args(["/T", "/F", "/PID", &pid.to_string()])
                .output()
        })
        .await
        .map_err(|e| std::io::Error::other(e))??;
        if !out.status.success() {
            let _ = child.kill();
        }
        let _ = child.wait();
        Ok(())
    }
}
