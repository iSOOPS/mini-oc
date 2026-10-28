//! 应用级 `.env` 持久化：应用数据目录下 `.env` 文件（0600）。
//!
//! 受管 key：`SERVER_URL` / `REMEMBER_PASSWORD` / `PASSWORD`。
//!
//! docker 逻辑（沿用 mini-oc-web 约定「显式环境变量永远优先」）：
//! 若进程 env 注入了对应 `MINI_OC_*` 变量（容器部署由 Dockerfile ENV 注入），
//! 读时以 env 为准；写时不落文件（容器文件系统可能只读，且注入值即真实值）。
//! 文件中不受管的内容（注释、其他 key）写入时原样保留。

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// env 文件名。
pub const ENV_FILE: &str = ".env";
/// 默认服务器地址（文件无值且无 env 注入时兜底）。
pub const DEFAULT_SERVER_URL: &str = "https://oc.isoops.com";

/// env 受管配置。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AppEnv {
    /// 服务器访问地址
    pub server_url: String,
    /// 是否记住密码（勾选状态）
    pub remember_password: bool,
    /// 密码明文（仅 remember_password = true 时保留）
    pub password: String,
}

impl Default for AppEnv {
    fn default() -> Self {
        Self {
            server_url: String::new(),
            remember_password: false,
            password: String::new(),
        }
    }
}

/// env 文件路径（与 config.json 同目录，MINI_OC_HOME 覆盖逻辑一致）。
#[flutter_rust_bridge::frb(ignore)]
pub fn env_path() -> PathBuf {
    let home = std::env::var("MINI_OC_HOME").ok().map(PathBuf::from);
    home.unwrap_or_else(super::super::config::default_data_dir)
        .join(ENV_FILE)
}

/// 解析单行 key=value（去引号、去空白；注释/空行返回 None）。
fn parse_line(line: &str) -> Option<(String, String)> {
    let trimmed = line.trim();
    if trimmed.is_empty() || trimmed.starts_with('#') {
        return None;
    }
    let (k, v) = trimmed.split_once('=')?;
    let key = k.trim().to_string();
    let value = v.trim().trim_matches('"').trim_matches('\'').to_string();
    Some((key, value))
}

/// 读 env 文件（文件缺失/损坏 → 默认值，不报错）。
#[flutter_rust_bridge::frb(ignore)]
pub fn load_app_env_from(path: &std::path::Path) -> AppEnv {
    let mut out = AppEnv::default();
    let Ok(raw) = std::fs::read_to_string(path) else {
        return out;
    };
    for line in raw.lines() {
        if let Some((k, v)) = parse_line(line) {
            match k.as_str() {
                "SERVER_URL" => out.server_url = v,
                "REMEMBER_PASSWORD" => out.remember_password = v == "true",
                "PASSWORD" => out.password = v,
                _ => {}
            }
        }
    }
    out
}

/// 进程 env 是否注入了任一受管变量（docker 场景判定）。
fn env_vars_injected() -> bool {
    [
        "MINI_OC_SERVER_URL",
        "MINI_OC_REMEMBER_PASSWORD",
        "MINI_OC_PASSWORD",
    ]
    .iter()
    .any(|k| std::env::var(k).map(|v| !v.is_empty()).unwrap_or(false))
}

fn env_var(k: &str) -> Option<String> {
    std::env::var(k).ok().filter(|v| !v.is_empty())
}

/// 读应用 env：进程 env 注入（docker）→ 逐项优先；否则读文件。
pub fn load_app_env() -> AppEnv {
    let mut cfg = load_app_env_from(&env_path());
    if env_vars_injected() {
        if let Some(v) = env_var("MINI_OC_SERVER_URL") {
            cfg.server_url = v;
        }
        if let Some(v) = env_var("MINI_OC_REMEMBER_PASSWORD") {
            cfg.remember_password = v == "true";
        }
        if let Some(v) = env_var("MINI_OC_PASSWORD") {
            cfg.password = v;
        }
    }
    cfg
}

/// 写应用 env：合并更新受管 key（未知行保留），0600 权限。
///
/// docker 逻辑：进程 env 已注入时**不写文件**（注入值即真实值，容器文件系统可能只读）。
pub fn save_app_env(cfg: &AppEnv) -> Result<(), String> {
    if env_vars_injected() {
        tracing::info!("检测到 MINI_OC_* 环境变量注入（docker），跳过 env 文件写入");
        return Ok(());
    }
    save_app_env_to(&env_path(), cfg)
}

/// 写 env 文件到指定路径（测试与实现共用）。
#[flutter_rust_bridge::frb(ignore)]
pub fn save_app_env_to(path: &std::path::Path, cfg: &AppEnv) -> Result<(), String> {
    let lines: Vec<String> = match std::fs::read_to_string(path) {
        Ok(raw) => raw.lines().map(str::to_string).collect(),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Vec::new(),
        Err(e) => return Err(format!("读取 env 文件失败: {e}")),
    };

    let updates: Vec<(String, String)> = vec![
        ("SERVER_URL".into(), cfg.server_url.trim().to_string()),
        (
            "REMEMBER_PASSWORD".into(),
            cfg.remember_password.to_string(),
        ),
        ("PASSWORD".into(), cfg.password.clone()),
    ];

    let mut out: Vec<String> = Vec::with_capacity(lines.len() + 4);
    let mut seen: Vec<bool> = Vec::with_capacity(updates.len());
    for _ in &updates {
        seen.push(false);
    }
    let mut managed_section_pending = true;

    for line in lines {
        let is_managed = updates
            .iter()
            .position(|(k, _)| parse_line(&line).map(|(pk, _)| pk == *k).unwrap_or(false));
        match is_managed {
            Some(i) => {
                // 受管 key 就地更新（password 空时直接删除该行）
                if !updates[i].1.is_empty() {
                    out.push(format!("{}={}", updates[i].0, updates[i].1));
                }
                seen[i] = true;
            }
            None => out.push(line),
        }
    }

    // 追加文件中原本不存在的受管 key（值非空才写，避免空行噪音）
    let mut header_pushed = false;
    for (i, (k, v)) in updates.iter().enumerate() {
        if !seen[i] && !v.is_empty() {
            if managed_section_pending {
                out.push(String::new());
                out.push("# --- app env (managed) ---".into());
                managed_section_pending = false;
                header_pushed = true;
            }
            out.push(format!("{k}={v}"));
        }
    }
    let _ = header_pushed;

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("创建 env 目录失败: {e}"))?;
    }
    let mut body = out.join("\n");
    if !body.is_empty() {
        body.push('\n');
    }
    std::fs::write(path, body).map_err(|e| format!("写入 env 文件失败: {e}"))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
            .map_err(|e| format!("设置 env 权限失败: {e}"))?;
    }
    Ok(())
}

/// 生效的服务器地址：env（文件或注入）→ config.json 兜底（老用户升级迁移）→ 默认值。
pub fn server_url_effective() -> String {
    let v = load_app_env().server_url;
    let v = if v.trim().is_empty() {
        super::super::config::load_config().server_url
    } else {
        v
    };
    if v.trim().is_empty() {
        DEFAULT_SERVER_URL.into()
    } else {
        v
    }
}
