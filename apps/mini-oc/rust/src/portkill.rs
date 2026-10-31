//! 端口占用处理：查占用进程 + 杀占用者（I10）。
//!
//! 交互流程：启动服务遇端口占用（错误带 [`PORT_ERR_PREFIX`] 标识）→
//! UI 弹框展示占用进程（`port_occupant`）→ 用户确认 → `kill_port` → 重试启动。
//!
//! 实现备注：
//! - 查占用者：`lsof -tPni :{port} -sTCP:LISTEN`（macOS；Windows 用 netstat）；
//! - 杀进程：SIGTERM → 2s 宽限 → SIGKILL 兜底（复用 supervisor 的终止策略）；
//! - 全程不依赖外部 crate（不引 lsof/sysinfo，保持依赖面不变）。

use serde::{Deserialize, Serialize};
use std::net::TcpListener;
use std::process::Command;
use std::time::{Duration, Instant};

/// 端口占用错误的机器可读前缀（Dart 侧据此弹框）。
pub const PORT_ERR_PREFIX: &str = "PORT_IN_USE:";

/// 端口占用者摘要（弹框展示用）。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PortOccupant {
    pub pid: u32,
    /// 进程名（取自 ps，展示用；获取失败时为空串）
    pub name: String,
}

/// 端口是否被占用（仅判定，不查进程）。
pub fn is_port_in_use(port: u16) -> bool {
    TcpListener::bind(("127.0.0.1", port)).is_err()
}

/// 查占用端口的监听进程；无占用/查询失败返回 None。
pub fn port_occupant(port: u16) -> Option<PortOccupant> {
    let pid = find_listener_pid(port)?;
    let name = process_name(pid).unwrap_or_default();
    Some(PortOccupant { pid, name })
}

/// 杀掉端口占用者并等待端口释放。
///
/// 步骤：查占用者 → SIGTERM → 宽限（2s）→ SIGKILL → 等端口可 bind（3s）。
/// 查无占用者（竞态退出）或杀后端口仍被占 → Err。
pub fn kill_port(port: u16) -> Result<(), String> {
    let Some(occ) = port_occupant(port) else {
        return Err(format!("端口 {port} 无占用进程（可能刚被释放）"));
    };
    kill_pid(occ.pid).map_err(|e| format!("结束进程 {} 失败: {e}", occ.pid))?;
    // 等端口真正释放（内核回收 + TIME_WAIT 不影响 listen，但保险轮询）
    let deadline = Instant::now() + Duration::from_secs(3);
    while Instant::now() < deadline {
        if TcpListener::bind(("127.0.0.1", port)).is_ok() {
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    Err(format!("进程已结束但端口 {port} 仍被占用（可能是其他进程接管）"))
}

/// 查监听端口的 PID（无/多个监听者取第一个；lsof 失败返回 None）。
fn find_listener_pid(port: u16) -> Option<u32> {
    #[cfg(unix)]
    {
        let out = Command::new("lsof")
            .args(["-tPni", &format!(":{port}"), "-sTCP:LISTEN"])
            .output()
            .ok()?;
        if !out.status.success() {
            return None;
        }
        String::from_utf8_lossy(&out.stdout)
            .lines()
            .next()
            .and_then(|l| l.trim().parse().ok())
    }
    #[cfg(target_os = "windows")]
    {
        // netstat -ano:  Proto Local Foreign State PID
        let out = Command::new("netstat")
            .args(["-ano", "-p", "TCP"])
            .output()
            .ok()?;
        let needle = format!(":{port}");
        for line in String::from_utf8_lossy(&out.stdout).lines() {
            let cols: Vec<&str> = line.split_whitespace().collect();
            if cols.len() >= 5
                && cols[3].eq_ignore_ascii_case("LISTENING")
                && cols[1].ends_with(&needle)
            {
                return cols[4].parse().ok();
            }
        }
        None
    }
}

/// 进程名（ps 按PID；失败返回 None）。
fn process_name(pid: u32) -> Option<String> {
    #[cfg(unix)]
    {
        let out = Command::new("ps")
            .args(["-p", &pid.to_string(), "-o", "comm="])
            .output()
            .ok()?;
        if !out.status.success() {
            return None;
        }
        let name = String::from_utf8_lossy(&out.stdout).trim().to_string();
        if name.is_empty() { None } else { Some(name) }
    }
    #[cfg(target_os = "windows")]
    {
        let out = Command::new("tasklist")
            .args(["/FI", &format!("PID eq {pid}"), "/FO", "CSV", "/NH"])
            .output()
            .ok()?;
        let line = String::from_utf8_lossy(&out.stdout).lines().next()?;
        let name = line.split(',').next()?.trim_matches('"');
        if name.is_empty() { None } else { Some(name.to_string()) }
    }
}

/// 平台终止进程：SIGTERM → 宽限 → SIGKILL（与 supervisor 同策略，同步版）。
fn kill_pid(pid: u32) -> Result<(), String> {
    #[cfg(unix)]
    {
        use nix::sys::signal::{kill, Signal};
        use nix::unistd::Pid;
        let nix_pid = Pid::from_raw(pid as i32);
        kill(nix_pid, Signal::SIGTERM).map_err(|e| format!("SIGTERM: {e}"))?;
        let deadline = Instant::now() + Duration::from_secs(2);
        while Instant::now() < deadline {
            // SIGNULL（signal=None）探测存活：ESRCH = 进程已退出
            if kill(nix_pid, None).is_err() {
                return Ok(()); // 进程已不存在
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        kill(nix_pid, Signal::SIGKILL).map_err(|e| format!("SIGKILL: {e}"))?;
        Ok(())
    }
    #[cfg(target_os = "windows")]
    {
        let out = Command::new("taskkill")
            .args(["/T", "/F", "/PID", &pid.to_string()])
            .output()
            .map_err(|e| e.to_string())?;
        if out.status.success() {
            Ok(())
        } else {
            Err(String::from_utf8_lossy(&out.stderr).to_string())
        }
    }
}
