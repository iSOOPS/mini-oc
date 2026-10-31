//! 端口占用处理（I10）：查占用者 + 杀占用者 + 错误标识。
//!
//! 场景：启动本地/云服务时 `ensure_port_free` 失败。旧 UI 只能 SnackBar 报错
//! 让用户手动换端口；新交互——弹框展示占用进程（PID/名称），用户确认后
//! 杀掉占用者并自动重试启动。
//!
//! 契约：
//! - `PORT_ERR_PREFIX`（"PORT_IN_USE:"）：`start_local` 端口占用错误的机器可读
//!   标识，Dart 侧据此识别弹框场景；
//! - `port_occupant(port)`：返回占用该端口的监听进程（PID + 进程名）；
//!   找不到（竞态：进程刚好退出/权限不可见）返回 None；
//! - `kill_port(port)`：查占用者 → SIGTERM → 宽限期 → SIGKILL 兜底 →
//!   端口释放返回 Ok。杀不动/查无占用者返回 Err。

use rust_lib_mini_oc::portkill::{kill_port, port_occupant, PORT_ERR_PREFIX};
use std::net::TcpListener;
use std::process::{Command, Stdio};
use std::time::Duration;

/// 起一个占住端口的子进程（python http.server，macOS 自带 python3）。
/// 若端口已被遗留进程占用（历史测试失败残留等），先清场再起，测试自愈。
fn spawn_port_holder(port: u16) -> std::process::Child {
    let _ = kill_port(port); // 清场失败则由后续断言暴露
    Command::new("/usr/bin/python3")
        .args(["-m", "http.server", &port.to_string(), "--bind", "127.0.0.1"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("python3 启动失败（测试环境需 macOS 自带 python3）")
}

/// 等待端口真正被监听（进程启动 → bind 有延迟）。
fn wait_listening(port: u16, timeout: Duration) {
    let deadline = std::time::Instant::now() + timeout;
    loop {
        if TcpListener::bind(("127.0.0.1", port)).is_err() {
            return; // bind 失败 = 已被占用
        }
        if std::time::Instant::now() > deadline {
            panic!("等待端口 {port} 被监听超时");
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

#[test]
fn port_occupant_finds_listener_process() {
    let port = 22000 + (std::process::id() % 1000) as u16;
    let mut child = spawn_port_holder(port);
    wait_listening(port, Duration::from_secs(5));

    let occ = port_occupant(port).expect("应找到占用进程");
    assert_eq!(occ.pid, child.id(), "占用者 PID 应是监听进程");
    assert!(!occ.name.is_empty(), "进程名非空");

    let _ = child.kill();
    let _ = child.wait();
}

#[test]
fn port_occupant_none_when_port_free() {
    // 找一个确定空闲的端口（bind 后立即释放，TIME_WAIT 不影响 listen）
    let port = {
        let l = TcpListener::bind(("127.0.0.1", 0)).expect("bind 随机端口");
        let p = l.local_addr().unwrap().port();
        drop(l);
        p
    };
    assert!(port_occupant(port).is_none(), "空闲端口无占用者");
}

#[test]
fn kill_port_releases_occupied_port() {
    let port = 23000 + (std::process::id() % 1000) as u16;
    let mut child = spawn_port_holder(port);
    wait_listening(port, Duration::from_secs(5));

    kill_port(port).expect("杀端口占用者应成功");

    // 进程应已退出（SIGTERM/SIGKILL 后 wait 收尸）
    let status = child.wait().expect("wait 子进程");
    assert!(!status.success(), "占用进程应被信号杀死而非正常退出");

    // 端口应可再次 bind
    let l = TcpListener::bind(("127.0.0.1", port));
    assert!(l.is_ok(), "杀掉占用者后端口应可绑定: {:?}", l.err());
}

#[test]
fn kill_port_err_when_no_occupant() {
    let port = {
        let l = TcpListener::bind(("127.0.0.1", 0)).expect("bind 随机端口");
        let p = l.local_addr().unwrap().port();
        drop(l);
        p
    };
    assert!(kill_port(port).is_err(), "无占用者时 kill_port 应 Err");
}

#[test]
fn port_err_prefix_is_machine_readable() {
    // Dart 侧按此前缀识别「端口占用」错误 → 弹框；改前缀会静默破坏 UI 判定
    assert_eq!(PORT_ERR_PREFIX, "PORT_IN_USE:");
}
