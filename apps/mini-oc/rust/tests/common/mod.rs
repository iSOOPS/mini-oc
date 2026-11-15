//! 跨平台测试替身工具（Windows / macOS 共用）。
//!
//! macOS 用 `/bin/sleep`、`/usr/bin/python3`、`/bin/bash` 等系统自带的 Unix
//! 工具做进程替身；Windows 没有这些，故在此统一提供平台无关的等价替身，
//! 避免测试套件在 Windows 上失败。生产代码不受影响。

use std::process::Command;

/// 返回一个「存活约 secs 秒」的进程命令：
/// - Unix：`/bin/sleep {secs}`
/// - Windows：`ping -n {secs} 127.0.0.1`（约 1 秒 1 个 ICMP 请求，等效 sleep）
pub fn sleeper(secs: u64) -> Command {
    #[cfg(unix)]
    {
        let mut c = Command::new("/bin/sleep");
        c.arg(secs.to_string());
        c
    }
    #[cfg(target_os = "windows")]
    {
        let mut c = Command::new("ping");
        c.args(["-n", &secs.to_string(), "127.0.0.1"]);
        c
    }
}

/// 返回一个「打印两行日志后保持存活约 60 秒」的进程命令，用于验证子进程
/// stdout 逐行进入日志管道：
/// - Unix：`bash -c "echo ...; echo ...; sleep 60"`
/// - Windows：`cmd /c "echo ... & echo ... & ping -n 60 127.0.0.1 > nul"`
#[allow(dead_code)] // 共享测试替身：仅被部分 test binary（fix_pass_test）引用
pub fn echo_then_sleep(port: u16) -> Command {
    #[cfg(unix)]
    {
        let mut c = Command::new("/bin/bash");
        c.args([
            "-c",
            &format!("echo 'serve ready on {port}'; echo 'secondary line'; sleep 60"),
        ]);
        c
    }
    #[cfg(target_os = "windows")]
    {
        let mut c = Command::new("cmd");
        c.args([
            "/c",
            &format!(
                "echo serve ready on {port}& echo secondary line& ping -n 60 127.0.0.1 > nul"
            ),
        ]);
        c
    }
}