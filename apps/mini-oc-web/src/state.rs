//! 应用状态与环境配置。

use crate::auth::RateLimiter;
use crate::error::AppResult;
use crate::sb::SbClient;
use rand::RngCore;
use std::path::PathBuf;

#[derive(Debug)]
pub struct AppState {
    pub sb: SbClient,
    pub cookie_key: Vec<u8>,
    pub rate: RateLimiter,
    pub started_at: chrono::DateTime<chrono::Utc>,
}

impl AppState {
    /// 环境变量构造（生产入口）。
    ///
    /// - `SB_BASE_URL` / `SB_USER` / `SB_PASSWORD`：SilverBullet 连接（SB_PASSWORD 兼作超管密码）
    /// - `COOKIE_KEY_PATH`：HMAC 密钥路径（首启生成 32 字节，0600）
    pub fn from_env() -> AppResult<Self> {
        let base = std::env::var("SB_BASE_URL").unwrap_or_else(|_| "http://127.0.0.1:3000".into());
        let user = std::env::var("SB_USER").unwrap_or_else(|_| "admin".into());
        let pass = std::env::var("SB_PASSWORD").unwrap_or_default();
        let key_path = std::env::var("COOKIE_KEY_PATH").unwrap_or_else(|_| "/data/cookie_key".into());
        Self::new(&base, &user, &pass, PathBuf::from(key_path))
    }

    pub fn new(sb_base: &str, sb_user: &str, sb_pass: &str, cookie_key_path: PathBuf) -> AppResult<Self> {
        let cookie_key = load_or_create_key(&cookie_key_path)?;
        Self::with_key(sb_base, sb_user, sb_pass, cookie_key)
    }

    /// 测试用：显式注入 cookie 密钥（确定性签名）。
    pub fn with_key(sb_base: &str, sb_user: &str, sb_pass: &str, cookie_key: Vec<u8>) -> AppResult<Self> {
        Ok(Self {
            sb: SbClient::new(sb_base, sb_user, sb_pass)?,
            cookie_key,
            rate: RateLimiter::new(5, std::time::Duration::from_secs(600)),
            started_at: chrono::Utc::now(),
        })
    }
}

/// 加载或首次生成 HMAC cookie 密钥（32 字节随机，0600 权限）。
fn load_or_create_key(path: &PathBuf) -> AppResult<Vec<u8>> {
    if path.exists() {
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
