//! 系统终端启动器：新窗口运行 `opencode <dir> --server <url> [--session <id>]`。
//!
//! 凭据零泄露契约（spec §8 + 计划 Review Focus #4）：
//! - **命令行（display_command）绝不含 key**——ps/终端历史不可见；
//! - macOS Terminal/iTerm 不继承进程 env（launchd 会话），凭据经 **0600 env 文件**传递，
//!   launcher 脚本仅 `source` 引用（脚本内容亦无 key）；
//! - Windows `cmd /c start` 的子进程继承 env 链，直接用 env var（V1 已验证路径）；
//! - 过期 env 文件由 `cleanup_env_files` 回收（启动新终端时调用）。

use std::path::{Path, PathBuf};

/// 生成的一个文件（脚本/env）。
pub struct PlanFile {
    pub path: PathBuf,
    pub content: String,
    /// 文件权限模式（unix）
    pub mode: u32,
}

/// 终端启动计划。
pub struct TerminalPlan {
    /// 执行的命令（进入 osascript/cmd 的部分；不含凭据）
    pub display_command: String,
    pub files: Vec<PlanFile>,
}

/// 临时文件前缀。
const ENV_PREFIX: &str = "oc-term-";
/// env 文件过期阈值（1 小时）。
const ENV_STALE_SECS: u64 = 3600;

/// 构建启动计划（写入临时文件但不执行——测试与执行共用）。
pub fn build_terminal_plan(
    binary: &str,
    server_url: &str,
    directory: &str,
    session: Option<&str>,
    tmp_dir: Option<&Path>,
    user_key: &str,
) -> Result<TerminalPlan, String> {
    let tmp = tmp_dir
        .map(|p| p.to_path_buf())
        .unwrap_or_else(std::env::temp_dir);
    let nonce = format!("{}{}", std::process::id(), chrono::Utc::now().timestamp_millis());

    #[cfg(unix)]
    {
        build_macos(binary, server_url, directory, session, &tmp, &nonce, user_key)
    }
    #[cfg(target_os = "windows")]
    {
        build_windows(binary, server_url, directory, session, &tmp, &nonce)
    }
}

#[cfg(unix)]
fn build_macos(
    binary: &str,
    server_url: &str,
    directory: &str,
    session: Option<&str>,
    tmp: &Path,
    nonce: &str,
    user_key: &str,
) -> Result<TerminalPlan, String> {
    // 1) env 文件（0600，含唯一明文点——同机同用户可读是接受的威胁模型边界）
    let env_path = tmp.join(format!("{ENV_PREFIX}{nonce}.env"));
    let env_content = format!("export OPENCODE_PASSWORD='{user_key}'\n");

    // 2) launcher.sh：source env 文件后 exec（bash PID 即 opencode PID）
    let script_path = tmp.join(format!("{ENV_PREFIX}{nonce}.sh"));
    let session_arg = session.map(|s| format!(" --session {s}")).unwrap_or_default();
    let script_content = format!(
        "#!/bin/bash\n# mini-oc 终端启动器（自动生成，勿手改）\nsource '{env}'\nexec '{binary}' --server '{url}'{session_arg} '{directory}'\n",
        env = env_path.display(),
        binary = binary,
        url = server_url,
        session_arg = session_arg,
        directory = directory,
    );

    std::fs::write(&env_path, &env_content).map_err(|e| format!("写入 env 失败: {e}"))?;
    std::fs::write(&script_path, &script_content).map_err(|e| format!("写入脚本失败: {e}"))?;
    set_mode(&env_path, 0o600);
    set_mode(&script_path, 0o700);

    // 3) display_command：探测 iTerm2（运行中→新 tab），否则 Terminal.app
    //    仅含脚本路径——无凭据。
    let iterm_running = std::process::Command::new("pgrep")
        .arg("-x")
        .arg("iTerm2")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    let escaped_script = script_path.display().to_string().replace('\'', "'\\''");
    let display_command = if iterm_running {
        format!(
            "osascript -e 'tell application \"iTerm2\" to activate' -e 'tell application \"iTerm2\" to create window with default profile' -e 'tell application \"iTerm2\" to tell current session of current window to write text \"bash {escaped_script}\"'",
        )
    } else {
        format!(
            "osascript -e 'tell application \"Terminal\" to activate' -e 'tell application \"Terminal\" to do script \"bash {escaped_script}\"'",
        )
    };

    Ok(TerminalPlan {
        display_command,
        files: vec![
            PlanFile { path: env_path, content: env_content, mode: 0o600 },
            PlanFile { path: script_path, content: script_content, mode: 0o700 },
        ],
    })
}

#[cfg(target_os = "windows")]
fn build_windows(
    binary: &str,
    server_url: &str,
    directory: &str,
    session: Option<&str>,
    tmp: &Path,
    _nonce: &str,
) -> Result<TerminalPlan, String> {
    // env 经进程链继承（cmd /c start 子进程继承 env），launcher.ps1 只读 $env:
    let script_path = tmp.join("mini-oc-launcher.ps1");
    let session_arg = session.map(|s| format!(" --session {s}")).unwrap_or_default();
    let script_content = format!(
        "# mini-oc 终端启动器（自动生成，勿手改）\nSet-Location -LiteralPath '{directory}'\n& '{binary}' --server '{url}'{session_arg} .\n",
    );
    std::fs::write(&script_path, &script_content).map_err(|e| format!("写入脚本失败: {e}"))?;
    let display_command = format!(
        "cmd /c start \"mini-oc\" pwsh -NoExit -ExecutionPolicy Bypass -File \"{}\"",
        script_path.display()
    );
    Ok(TerminalPlan {
        display_command,
        files: vec![PlanFile { path: script_path, content: script_content, mode: 0o644 }],
    })
}

/// 执行启动计划（spawn display_command；env 继承 + Windows 注入 OPENCODE_PASSWORD）。
pub fn open_in_terminal(binary: &str, server_url: &str, directory: &str, session: Option<&str>, user_key: &str) -> Result<(), String> {
    // 先清理过期 env 文件
    if let Err(e) = cleanup_env_files(&std::env::temp_dir()) {
        tracing::warn!(%e, "清理过期终端 env 文件失败");
    }
    let plan = build_terminal_plan(binary, server_url, directory, session, None, user_key)?;
    let mut cmd = shell_command(&plan.display_command);
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        cmd.env("OPENCODE_PASSWORD", user_key); // 进程链继承
        cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW（防 shim 闪窗）
    }
    #[cfg(unix)]
    {
        let _ = user_key; // macOS 经 env 文件，无需进程 env
    }
    cmd.spawn()
        .map_err(|e| format!("打开终端失败: {e}"))?;
    Ok(())
}

#[cfg(unix)]
fn shell_command(display: &str) -> std::process::Command {
    let mut c = std::process::Command::new("/bin/bash");
    c.arg("-c").arg(display);
    c
}

#[cfg(target_os = "windows")]
fn shell_command(display: &str) -> std::process::Command {
    // display_command 本身就是 cmd /c ... 形式
    let mut parts = display.splitn(4, ' ');
    let mut c = std::process::Command::new(parts.next().unwrap_or("cmd"));
    for p in parts {
        c.arg(p);
    }
    c
}

#[cfg(unix)]
fn set_mode(path: &Path, mode: u32) {
    use std::os::unix::fs::PermissionsExt;
    let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode));
}

/// 清理过期的终端 env 文件（>1 小时）。
pub fn cleanup_env_files(dir: &Path) -> std::io::Result<()> {
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return Ok(()),
    };
    let now = std::time::SystemTime::now();
    for entry in entries.flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name.starts_with(ENV_PREFIX) && (name.ends_with(".env") || name.ends_with(".sh")) {
            let stale = entry
                .metadata()
                .ok()
                .and_then(|m| m.modified().ok())
                .and_then(|t| now.duration_since(t).ok())
                .map(|d| d > std::time::Duration::from_secs(ENV_STALE_SECS))
                .unwrap_or(false);
            if stale {
                let _ = std::fs::remove_file(entry.path());
            }
        }
    }
    Ok(())
}
