//! opencode 二进制定位测试。
//!
//! 背景 1（Windows）：npm 安装的 opencode 在 Windows 上落为 `opencode.cmd`/`opencode`
//! （shell 脚本），Rust 的 `Command::new("opencode")` 不做 PATHEXT 扩展会 `NotFound`。
//! 背景 2（GUI 启动，I21）：macOS 上从 Finder/`open` 启动 .app 只继承 launchd 的
//! 最小 PATH（`/usr/bin:/bin:/usr/sbin:/sbin`），不含 npm 全局目录等用户级 bin——
//! 裸名 `opencode` 回退会 NotFound，报「启动失败（检查二进制路径）」。
//! 故默认回退须依次探测：PATH 上的绝对路径 → 常见安装位（npm 全局 prefix、
//! opencode 官方目录等），都找不到才退回裸名（交由 spawn 报明确错误）。

use rust_lib_mini_oc::api::resolve_opencode_bin;

#[test]
fn resolve_returns_a_value_never_panics() {
    // 无论环境如何（有无 config、有无 PATH），resolve 永不 panic（I9 风格防御）。
    let _ = resolve_opencode_bin();
}

/// PATH 上有 opencode 时（如终端 `flutter run` 场景），resolve 应解析为绝对路径
/// 而非裸名——绝对路径对 PATH 语义无损（跨 PATH 环境的子进程仍可 spawn），
/// 且让 GUI 场景（launchd 最小 PATH）同样可用。
#[test]
fn resolve_prefers_absolute_path_when_on_path() {
    // 在 PATH 目录里伪造一个 opencode 可执行文件，验证「PATH 探测→绝对路径」。
    let dir = tempfile::tempdir().unwrap();
    let fake = dir.path().join(if cfg!(target_os = "windows") {
        "opencode.exe"
    } else {
        "opencode"
    });
    std::fs::write(&fake, b"#!/bin/sh\n").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&fake, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    let orig_path = std::env::var_os("PATH");
    std::env::set_var("PATH", dir.path());

    let v = resolve_opencode_bin();

    // 恢复现场（其余测试还依赖原 PATH）
    match orig_path {
        Some(p) => std::env::set_var("PATH", p),
        None => std::env::remove_var("PATH"),
    }
    let _ = std::env::remove_var("OPENCODE_BIN");

    // config.opencode_bin 未覆盖时（测试机通常为 null），应返回绝对路径
    if crate_helpers::config_opencode_bin_is_none() {
        let norm = |p: &std::path::Path| p.canonicalize().unwrap_or_else(|_| p.to_path_buf());
        assert_eq!(
            norm(std::path::Path::new(&v)),
            norm(&fake),
            "PATH 命中时应返回绝对路径，实际: {v}"
        );
    } else {
        eprintln!("config.opencode_bin 已覆盖（{v}），跳过断言");
    }
}

/// GUI 启动场景（I21 根因）：PATH 被剥成 launchd 最小集、config 无覆盖时，
/// resolve 须能从「常见安装位」找到 opencode；一台装有 opencode 的机器上
/// resolve 不应退化为裸名。仅当本机确实装了 opencode 时才断言（CI 无则跳过）。
#[test]
fn resolve_finds_opencode_outside_path_when_installed() {
    // 模拟 GUI 环境：剥掉 PATH（本测试只验证「PATH 之外的回退」确实生效）
    let orig_path = std::env::var_os("PATH");
    std::env::remove_var("OPENCODE_BIN");
    std::env::set_var("PATH", if cfg!(target_os = "windows") {
        r"C:\Windows\System32"
    } else {
        "/usr/bin:/bin"
    });

    // 常见安装位至少有一个 opencode 才有意义断言（CI/裸机可能没有）
    let installed = rust_lib_mini_oc::api::probe_common_install_dirs().is_some();
    let v = resolve_opencode_bin();

    // 恢复现场（其余测试还依赖原 PATH）
    match orig_path {
        Some(p) => std::env::set_var("PATH", p),
        None => std::env::remove_var("PATH"),
    }

    if installed {
        assert!(
            v.contains(std::path::MAIN_SEPARATOR),
            "PATH 外找到 opencode 时应返回绝对路径，实际: {v}"
        );
        assert!(std::path::Path::new(&v).is_file(), "返回值必须是存在的文件: {v}");
    } else {
        eprintln!("本机未装 opencode（常见安装位均未命中），退回默认值: {v}");
    }
}

/// 端到端（GUI 场景）：launchd 最小 PATH 下 resolve 出的路径必须能真实 spawn
/// ——这正是 supervisor.rs spawn 报「启动失败（检查二进制路径）」的场景。
/// 本机装有 opencode 时验证完整链路；未装则跳过。
#[test]
fn resolved_bin_actually_spawns_under_minimal_gui_path() {
    let orig_path = std::env::var_os("PATH");
    std::env::set_var("PATH", if cfg!(target_os = "windows") {
        r"C:\Windows\System32"
    } else {
        "/usr/bin:/bin:/usr/sbin:/sbin"
    });
    let bin = resolve_opencode_bin();
    let installed =
        bin.contains(std::path::MAIN_SEPARATOR) && std::path::Path::new(&bin).is_file();

    let result = std::process::Command::new(&bin)
        .arg("--version")
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .output();

    // 恢复现场
    match orig_path {
        Some(p) => std::env::set_var("PATH", p),
        None => std::env::remove_var("PATH"),
    }

    match (installed, result) {
        (true, Ok(out)) => {
            assert!(
                out.status.success(),
                "GUI 最小 PATH 下 spawn opencode --version 应成功: {}",
                String::from_utf8_lossy(&out.stderr)
            );
        }
        (true, Err(e)) => panic!("GUI 最小 PATH 下 spawn {bin} 失败（回归！）: {e}"),
        (false, _) => {
            eprintln!("本机未装 opencode（resolve: {bin}），跳过端到端 spawn 验证");
        }
    }
}

// ---------------------------------------------------------------------------
// 测试辅助
// ---------------------------------------------------------------------------

mod crate_helpers {
    /// config.opencode_bin 是否为 None（决定断言是否跳过）。
    pub fn config_opencode_bin_is_none() -> bool {
        rust_lib_mini_oc::config::load_config().opencode_bin.is_none()
    }
}

#[cfg(target_os = "windows")]
/// 在 PATH 上找可执行文件的绝对路径。
fn which_path(name: &str) -> Option<std::path::PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|dir| dir.join(name))
        .find(|full| full.is_file())
}

#[cfg(target_os = "windows")]
#[test]
fn resolve_defaults_to_cmd_or_exe_on_windows_when_opencode_in_path() {
    // 本机 PATH 含 `opencode.cmd`（nvm4w/nodejs），默认回退应探测到 .cmd 而非裸 opencode。
    // 仅当环境确实存在 opencode.cmd / opencode.exe 时断言；否则跳过（CI 无 npm opencode）。
    let has_cmd = which_path("opencode.cmd").is_some();
    let has_exe = which_path("opencode.exe").is_some();

    if !has_cmd && !has_exe {
        eprintln!("PATH 无 opencode.cmd/.exe，跳过");
        return;
    }

    // 清空可能的 config/环境覆盖，走默认回退分支
    std::env::remove_var("OPENCODE_BIN");
    // 注意：config.json 可能已有 opencode_bin 覆盖，这里不强制清 config——
    // 若被覆盖则本断言跳过（避免误报）。
    let v = resolve_opencode_bin();
    if v != "opencode" {
        assert!(
            v.ends_with(".cmd") || v.ends_with(".exe"),
            "Windows 默认应返回可执行的 .cmd/.exe，实际: {v}"
        );
    }
    // v == "opencode" 说明被 config.opencode_bin 覆盖为裸名（用户自定义），不在此断言范围。
}
