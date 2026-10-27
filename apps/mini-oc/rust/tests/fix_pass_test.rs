//! 修复 pass 补充：I4（子进程 stdout 接入日志）与 I7（rathole toml 权限）。

use rust_lib_mini_oc::logbuf::{push_log, recent_logs, clear_logs};
use rust_lib_mini_oc::rathole::{rathole_toml, write_settings_toml};
use rust_lib_mini_oc::account::DeviceBrief;
use rust_lib_mini_oc::supervisor::{SupConfig, Supervisor};
use std::sync::{Mutex, MutexGuard};
use std::time::Duration;

static ENV_LOCK: Mutex<()> = Mutex::new(());
fn lock() -> MutexGuard<'static, ()> {
    ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner())
}

fn device() -> DeviceBrief {
    DeviceBrief {
        name: "macbook-pro".into(),
        port: 9464,
        pctype: "macos".into(),
        bound: true,
        device_name: "X".into(),
        public_url: "https://oc.example.com".into(),
    }
}

#[test]
fn rathole_settings_toml_is_owner_only() {
    // I7 修复：toml 内含 user.key，必须 0600 落盘
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("global.toml");
    let toml = rathole_toml(&device(), "secret-key-123").unwrap();
    write_settings_toml(&path, &toml).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&path).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600, "rathole toml 权限 0600");
    }
    assert!(std::fs::read_to_string(&path).unwrap().contains("secret-key-123"));
}

#[tokio::test]
async fn child_stdout_streams_into_logbuf() {
    // I4 修复：子进程 stdout 逐行进入日志管道
    let _guard = lock();
    clear_logs();
    // serve 替身：打印两行后保持运行
    let sup = Supervisor::new(SupConfig {
        serve_command: Box::new(|port| {
            let mut c = std::process::Command::new("/bin/bash");
            c.args([
                "-c",
                &format!("echo 'serve ready on {port}'; echo 'secondary line'; sleep 60"),
            ]);
            c
        }),
        rathole_command: Box::new(|_| {
            let mut c = std::process::Command::new("/bin/bash");
            c.args(["-c", "echo 'rathole boot'; sleep 60"]);
            c
        }),
        probe: Box::new(|_| Box::pin(async { Ok("test".into()) })),
    });
    let port = 21000 + (std::process::id() % 500) as u16 * 2;
    sup.start_local(port).await.unwrap();
    // 日志异步汇入需要短暂等待
    for _ in 0..40 {
        tokio::time::sleep(Duration::from_millis(50)).await;
        let logs = recent_logs(100);
        if logs.iter().any(|l| l.source == "opencode" && l.message.contains("secondary line")) {
            sup.stop_all().await.unwrap();
            let all = recent_logs(100);
            assert!(all.iter().any(|l| l.source == "opencode" && l.message.contains("serve ready")));
            // 清理避免污染其他测试
            for l in all {
                let _ = l;
            }
            push_log("system", "info", "done");
            return;
        }
    }
    sup.stop_all().await.unwrap();
    panic!("子进程 stdout 未进入日志管道: {:?}", recent_logs(10));
}
