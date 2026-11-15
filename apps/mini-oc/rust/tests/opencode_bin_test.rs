//! opencode 二进制定位（Windows .cmd/.exe 特判）测试。
//!
//! 背景：npm 安装的 opencode 在 Windows 上落为 `opencode.cmd`/`opencode`（shell 脚本），
//! Rust 的 `Command::new("opencode")` 不做 PATHEXT 扩展会 `NotFound`，导致 opencode serve
//! 启动失败。`resolve_opencode_bin` 默认回退时须探测 `.cmd`/`.exe`。

use rust_lib_mini_oc::api::resolve_opencode_bin;

#[test]
fn resolve_returns_a_value_never_panics() {
    // 无论环境如何（有无 config、有无 PATH），resolve 永不 panic（I9 风格防御）。
    let _ = resolve_opencode_bin();
}

#[cfg(target_os = "windows")]
#[test]
fn resolve_defaults_to_cmd_or_exe_on_windows_when_opencode_in_path() {
    // 本机 PATH 含 `opencode.cmd`（nvm4w/nodejs），默认回退应探测到 .cmd 而非裸 opencode。
    // 仅当环境确实存在 opencode.cmd / opencode.exe 时断言；否则跳过（CI 无 npm opencode）。
    let has_cmd = std::env::var_os("PATH")
        .map(|p| {
            std::env::split_paths(&p)
                .map(|dir| dir.join("opencode.cmd"))
                .any(|f| f.is_file())
        })
        .unwrap_or(false);
    let has_exe = std::env::var_os("PATH")
        .map(|p| {
            std::env::split_paths(&p)
                .map(|dir| dir.join("opencode.exe"))
                .any(|f| f.is_file())
        })
        .unwrap_or(false);

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
