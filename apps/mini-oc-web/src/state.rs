//! 应用状态与环境配置。

use crate::auth::RateLimiter;
use crate::error::AppResult;
use crate::sb::SbClient;
use crate::snapshot::LiveCache;
use rand::RngCore;
use std::path::PathBuf;
use std::sync::Arc;

#[derive(Debug)]
pub struct AppState {
    pub sb: Arc<SbClient>,
    pub cookie_key: Vec<u8>,
    /// 超管登录密码（= SB_PASSWORD，常数时间比较用）
    pub admin_password: String,
    pub rate: RateLimiter,
    pub live_cache: Arc<LiveCache>,
    /// SPA 静态目录
    pub static_dir: PathBuf,
    pub started_at: chrono::DateTime<chrono::Utc>,
}

impl AppState {
    /// 环境变量构造（生产入口）。
    ///
    /// - `SB_BASE_URL` / `SB_USER` / `SB_PASSWORD`：SilverBullet 连接（SB_PASSWORD 兼作超管密码）
    /// - `COOKIE_KEY_PATH`：HMAC 密钥路径（首启生成 32 字节，0600）
    /// - `WEB_STATIC_DIR`：SPA 静态目录
    ///
    /// 显式变量永远优先（容器由 Dockerfile ENV 注入，行为不变）；
    /// 未设置时回退本地默认（见 [`resolve_local_defaults`]）。
    pub fn from_env() -> AppResult<Self> {
        let base = std::env::var("SB_BASE_URL").unwrap_or_else(|_| "http://127.0.0.1:3000".into());
        let user = std::env::var("SB_USER").unwrap_or_else(|_| "admin".into());
        let pass = std::env::var("SB_PASSWORD").unwrap_or_default();
        let (key_path, static_dir) = match (
            std::env::var("COOKIE_KEY_PATH").ok(),
            std::env::var("WEB_STATIC_DIR").ok(),
        ) {
            (Some(k), Some(s)) => (PathBuf::from(k), PathBuf::from(s)),
            (key_env, static_env) => {
                let cwd = std::env::current_dir()
                    .map_err(|e| crate::error::AppError::Internal(format!("获取工作目录: {e}")))?;
                let (local_key, local_static) = resolve_local_defaults(&cwd);
                let key_path = key_env.map(PathBuf::from).unwrap_or(local_key);
                let static_dir = static_env.map(PathBuf::from).unwrap_or(local_static);
                (key_path, static_dir)
            }
        };
        let mut state = Self::new(&base, &user, &pass, key_path)?;
        state.static_dir = static_dir;
        Ok(state)
    }

    pub fn new(sb_base: &str, sb_user: &str, sb_pass: &str, cookie_key_path: PathBuf) -> AppResult<Self> {
        let cookie_key = load_or_create_key(&cookie_key_path)?;
        Self::with_key(sb_base, sb_user, sb_pass, cookie_key)
    }

    /// 测试用：显式注入 cookie 密钥（确定性签名）。
    pub fn with_key(sb_base: &str, sb_user: &str, sb_pass: &str, cookie_key: Vec<u8>) -> AppResult<Self> {
        let static_dir = PathBuf::from(
            std::env::var("WEB_STATIC_DIR").unwrap_or_else(|_| "web/dist".into()),
        );
        Ok(Self {
            sb: Arc::new(SbClient::new(sb_base, sb_user, sb_pass)?),
            cookie_key,
            admin_password: sb_pass.to_string(),
            rate: RateLimiter::new(5, std::time::Duration::from_secs(600)),
            live_cache: Arc::new(LiveCache::new()),
            static_dir,
            started_at: chrono::Utc::now(),
        })
    }
}

/// 本地默认值：未设置环境变量时的回退逻辑（容器由 Dockerfile ENV 显式注入，不走此处）。
///
/// - cookie key：`<cwd>/data/cookie_key`（仓库 `data/` 已 gitignore）
/// - SPA 目录：按常见布局探测含 `index.html` 的 dist（仓库根 / apps/mini-oc-web / 本目录）；
///   探测失败回退 `web/dist`（页面 404 提示构建 SPA，服务照常启动）
pub fn resolve_local_defaults(cwd: &std::path::Path) -> (PathBuf, PathBuf) {
    const CANDIDATES: [&str; 4] = [
        "apps/mini-oc-web/web/dist",
        "apps/mini-oc-web/web",
        "web/dist",
        "web",
    ];
    let static_dir = CANDIDATES
        .iter()
        .map(|p| cwd.join(p))
        .find(|p| p.join("index.html").is_file())
        .unwrap_or_else(|| cwd.join("web/dist"));
    (cwd.join("data/cookie_key"), static_dir)
}

/// 加载或首次生成 HMAC cookie 密钥（32 字节随机，0600 权限）。
fn load_or_create_key(path: &PathBuf) -> AppResult<Vec<u8>> {    if path.exists() {
        let key = std::fs::read(path).map_err(|e| crate::error::AppError::Internal(format!("读取 cookie key: {e}")))?;
        if key.len() == 32 {
            return Ok(key);
        }
        return Err(crate::error::AppError::Internal(format!(
            "cookie key 文件损坏（长度 {} ≠ 32）：{}",
            key.len(),
            path.display()
        )));
    }
    let mut key = vec![0u8; 32];
    rand::thread_rng().fill_bytes(&mut key);
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)
            .map_err(|e| crate::error::AppError::Internal(format!("创建 key 目录: {e}")))?;
    }
    std::fs::write(path, &key)
        .map_err(|e| crate::error::AppError::Internal(format!("写入 cookie key: {e}")))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
            .map_err(|e| crate::error::AppError::Internal(format!("设置 key 权限: {e}")))?;
    }
    Ok(key)
}
