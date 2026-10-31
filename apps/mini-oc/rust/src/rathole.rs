//! rathole 客户端：配置生成（spec §5.3.1 契约）与二进制定位。
//!
//! 契约要点：
//! - **单 service**：service 名 = 设备条目 name 原样（与运维侧服务端 toml 对账）；
//! - token = user.key；
//! - local_addr = 127.0.0.1:<清单 port>（opencode serve）；
//! - remote_addr：public_url 无显式端口 → host:8999；带端口 → 原样。

use crate::account::DeviceBrief;
use mini_oc_core::tunnel::remote_addr_for_rathole;
use serde::Serialize;
use std::path::{Path, PathBuf};

#[derive(Serialize)]
struct Service {
    token: String,
    local_addr: String,
}

#[derive(Serialize)]
struct ClientSection {
    remote_addr: String,
    services: std::collections::BTreeMap<String, Service>,
}

/// 顶层文件结构（生成 `[client]` 表）。
#[derive(Serialize)]
struct File {
    client: ClientSection,
}

/// 生成 rathole 客户端配置 toml（写入 exe 同目录 rathole/settings/global.toml）。
pub fn rathole_toml(device: &DeviceBrief, user_key: &str) -> Result<String, String> {
    let remote_addr = remote_addr_for_rathole(&device.public_url)
        .map_err(|e| format!("public_url 非法: {e}"))?;
    let mut services = std::collections::BTreeMap::new();
    services.insert(
        device.name.clone(),
        Service {
            token: user_key.to_string(),
            local_addr: format!("127.0.0.1:{}", device.port),
        },
    );
    let cfg = File {
        client: ClientSection { remote_addr, services },
    };
    toml::to_string_pretty(&cfg).map_err(|e| format!("toml 序列化失败: {e}"))
}

/// 写 rathole 设置文件（内含 user.key —— **0600 落盘**，I7 修复）。
pub fn write_settings_toml(path: &Path, content: &str) -> Result<(), String> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("创建 rathole 目录失败: {e}"))?;
    }
    std::fs::write(path, content).map_err(|e| format!("写入 rathole 配置失败: {e}"))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
            .map_err(|e| format!("设置 rathole 配置权限失败: {e}"))?;
    }
    Ok(())
}

/// 二进制定位优先级：
/// 1. `RATHOLE_BIN` env
/// 2. macOS .app bundle：`MacOS/../Resources/assets/rathole/bin/<plat>/rathole`
///    （Flutter .app 结构：exe 在 Contents/MacOS/，build 脚本打包到
///    Contents/Resources/assets/——兄弟目录，I17）
/// 3. exe 同目录 `assets/rathole/bin/<plat>/rathole[.exe]`（随包分发的 bundle，
///    对齐 mini-oc-gui 的 target/<profile> 同目录布局）
/// 4. 开发模式：源码仓库 assets（向上找 apps/mini-oc/assets）
/// 5. PATH 上的 `rathole`
pub fn resolve_rathole_bin() -> Option<PathBuf> {
    if let Ok(p) = std::env::var("RATHOLE_BIN") {
        if !p.is_empty() {
            return Some(PathBuf::from(p));
        }
    }
    let platform = platform_dir();
    let exe_dir = std::env::current_exe().ok()?.parent()?.to_path_buf();
    resolve_rathole_bin_near(&exe_dir, &platform).or_else(which_rathole)
}

/// 纯路径探测（可测试）：给定 exe 所在目录与平台目录，按优先级查找。
/// 依次尝试：macOS .app Resources 布局 → exe 同目录 assets → 向上 5 级源码树。
pub fn resolve_rathole_bin_near(exe_dir: &Path, platform: &str) -> Option<PathBuf> {
    let name = rathole_name();
    // 1) macOS .app bundle：Contents/MacOS/../Resources/assets/...
    //    （仅当 exe_dir 结尾为 MacOS 时有意义， ../Resources 即 Contents/Resources）
    if cfg!(target_os = "macos") {
        if exe_dir.file_name().and_then(|s| s.to_str()) == Some("MacOS") {
            let candidate = exe_dir
                .parent()?
                .join("Resources/assets/rathole/bin")
                .join(platform)
                .join(name);
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    // 2) exe 同目录 assets bundle
    let candidate = exe_dir.join("assets/rathole/bin").join(platform).join(name);
    if candidate.is_file() {
        return Some(candidate);
    }
    // 3) 开发模式：源码仓库 assets（exe 在 target/debug 下，向上找 apps/mini-oc/assets）
    let mut dir = exe_dir;
    for _ in 0..5 {
        dir = dir.parent()?;
        let dev = dir.join("apps/mini-oc/assets/rathole/bin").join(platform).join(name);
        if dev.is_file() {
            return Some(dev);
        }
    }
    None
}

fn which_rathole() -> Option<PathBuf> {
    let name = rathole_name();
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|dir| dir.join(&name))
        .find(|full| full.is_file())
}

fn rathole_name() -> &'static str {
    if cfg!(target_os = "windows") { "rathole.exe" } else { "rathole" }
}

/// `<os>-<arch>` 目录名（与 V1 bundle 布局一致）。
fn platform_dir() -> String {
    let os = if cfg!(target_os = "macos") { "macos" } else { "windows" };
    let arch = if cfg!(target_arch = "aarch64") { "aarch64" } else { "x86_64" };
    format!("{os}-{arch}")
}
