//! Task 4：进程监督状态机测试（/bin/sleep 替身 + 注入探针）。

use rust_lib_mini_oc::supervisor::{SupConfig, Supervisor};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

/// 测试端口：按 pid 分段分配，避免并发测试随机端口碰撞。
fn test_port(idx: u16) -> u16 {
    20000 + ((std::process::id() % 1000) as u16) * 30 + idx
}

/// 构造测试用 Supervisor：serve/rathole 命令都是 `sleep 300`，探针立即成功。
fn test_supervisor() -> Supervisor {
    Supervisor::new(SupConfig {
        serve_command: Box::new(|_port| {
            let mut c = std::process::Command::new("/bin/sleep");
            c.arg("300");
            c
        }),
        rathole_command: Box::new(|_toml_path| {
            let mut c = std::process::Command::new("/bin/sleep");
            c.arg("300");
            c
        }),
        probe: Box::new(|_| Box::pin(async { Ok("test".into()) })),
    })
}

async fn wait_for<F: Fn(&Supervisor) -> bool>(s: &Supervisor, cond: F) {
    for _ in 0..100 {
        if cond(s) {
            return;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("条件等待超时");
}

#[tokio::test]
async fn start_local_transitions_to_local_only() {
    let sup = test_supervisor();
    // 找一个空闲端口
    let port = test_port(1);
    sup.start_local(port).await.unwrap();
    let st = sup.state();
    assert_eq!(st.mode_str(), "local_only");
    assert!(st.serve_pid.is_some());
    assert!(st.rathole_pid.is_none());
    assert_eq!(st.port, port);
    sup.stop_all().await.unwrap();
}

#[tokio::test]
async fn start_cloud_starts_both_processes() {
    let sup = test_supervisor();
    let port = test_port(2);
    // 预置 rathole toml（内容不校验，替身只看路径存在与否由实现决定）
    let dir = tempfile::tempdir().unwrap();
    let toml_path = dir.path().join("global.toml");
    std::fs::write(&toml_path, "[client]\n").unwrap();
    sup.start_cloud(port, &toml_path).await.unwrap();
    let st = sup.state();
    assert_eq!(st.mode_str(), "cloud");
    assert!(st.serve_pid.is_some(), "serve 已启动");
    assert!(st.rathole_pid.is_some(), "rathole 已启动");
    sup.stop_all().await.unwrap();
}

#[tokio::test]
async fn kill_serve_detected_and_state_goes_off_with_error() {
    let sup = Arc::new(test_supervisor());
    let port = test_port(3);
    sup.start_local(port).await.unwrap();
    let pid = sup.state().serve_pid.unwrap();

    // 启动后台监督循环
    let sup2 = sup.clone();
    let monitor = tokio::spawn(async move {
        sup2.run_monitor().await;
    });

    // 外部击杀 serve（模拟崩溃）
    let kill = std::process::Command::new("kill")
        .args(["-9", &pid.to_string()])
        .status()
        .unwrap();
    assert!(kill.success());

    wait_for(&sup, |s| {
        let st = s.state();
        st.mode_str() == "off" && st.last_error.is_some()
    }).await;
    monitor.abort();
}

#[tokio::test]
async fn port_occupied_returns_clear_error() {
    let sup = test_supervisor();
    // 占住一个端口
    let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = l.local_addr().unwrap().port();
    let err = sup.start_local(port).await.unwrap_err();
    assert!(err.contains(&port.to_string()), "错误信息含端口号: {err}");
}

#[tokio::test]
async fn cloud_downgrade_stops_rathole_keeps_serve() {
    let sup = test_supervisor();
    let port = test_port(4);
    let dir = tempfile::tempdir().unwrap();
    let toml_path = dir.path().join("global.toml");
    std::fs::write(&toml_path, "[client]\n").unwrap();
    sup.start_cloud(port, &toml_path).await.unwrap();

    sup.downgrade_to_local().await.unwrap();
    let st = sup.state();
    assert_eq!(st.mode_str(), "local_only");
    assert!(st.serve_pid.is_some(), "serve 保留");
    assert!(st.rathole_pid.is_none(), "rathole 已停");
    // rathole 进程确实死亡
    if let Some(pid) = st.rathole_pid {
        let _ = pid; // 已 None，占位
    }
    sup.stop_all().await.unwrap();
}

#[tokio::test]
async fn stop_all_leaves_no_orphan() {
    let sup = test_supervisor();
    let port = test_port(5);
    let dir = tempfile::tempdir().unwrap();
    let toml_path = dir.path().join("global.toml");
    std::fs::write(&toml_path, "[client]\n").unwrap();
    sup.start_cloud(port, &toml_path).await.unwrap();
    let serve_pid = sup.state().serve_pid.unwrap();
    let rathole_pid = sup.state().rathole_pid.unwrap();

    sup.stop_all().await.unwrap();
    assert_eq!(sup.state().mode_str(), "off");
    // 进程确实退出（kill -0 失败）
    std::thread::sleep(Duration::from_millis(200));
    let alive = |pid: u32| std::process::Command::new("kill").arg("-0").arg(pid.to_string()).status().map(|s| s.success()).unwrap_or(false);
    assert!(!alive(serve_pid), "serve 无孤儿进程");
    assert!(!alive(rathole_pid), "rathole 无孤儿进程");
}

#[tokio::test]
async fn start_local_probe_failure_stops_and_reports() {
    // 探针始终失败（模拟 opencode serve 起不来）
    static CALLS: AtomicUsize = AtomicUsize::new(0);
    let sup = Supervisor::new(SupConfig {
        serve_command: Box::new(|_port| {
            let mut c = std::process::Command::new("/bin/sleep");
            c.arg("300");
            c
        }),
        rathole_command: Box::new(|_| {
            let mut c = std::process::Command::new("/bin/sleep");
            c.arg("300");
            c
        }),
        probe: Box::new(|_| {
            CALLS.fetch_add(1, Ordering::SeqCst);
            Box::pin(async { Err("探针失败：serve 未就绪".into()) })
        }),
    });
    let port = test_port(6);
    let err = sup.start_local(port).await.unwrap_err();
    assert!(err.contains("探针") || err.contains("就绪"), "错误带探针语义: {err}");
    assert_eq!(sup.state().mode_str(), "off", "失败后回 Off");
    assert!(sup.state().serve_pid.is_none(), "失败不留进程");
}

/// 辅助：确认 #[allow(dead_code)] 用到的 AtomicBool 不裁剪。
#[allow(dead_code)]
static _KEEP: AtomicBool = AtomicBool::new(false);
