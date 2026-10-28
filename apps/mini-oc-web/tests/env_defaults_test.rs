//! 本地运行默认值逻辑：未设置环境变量时回退本地安全默认；
//! 容器由 Dockerfile ENV 显式注入路径，显式变量永远优先（原逻辑不变）。
//!
//! 注意：set_current_dir / set_var 是进程级全局状态，涉及它们的测试
//! 必须持 ENV_LOCK 串行执行；纯函数测试一律用绝对路径，不碰全局状态。

mod common;

use mini_oc_web::state::{resolve_local_defaults, AppState};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};

static ENV_LOCK: Mutex<()> = Mutex::new(());

fn lock_env() -> MutexGuard<'static, ()> {
    ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner())
}

/// 一次性临时目录，测试结束自动删除。
struct TempDir(PathBuf);

impl TempDir {
    fn new(tag: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("mini-oc-env-{}-{}", tag, std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        TempDir(dir)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// 切换进程 cwd 的守卫（须持 ENV_LOCK 使用）。
struct CwdGuard(PathBuf);

impl CwdGuard {
    fn enter(dir: &Path) -> Self {
        let original = std::env::current_dir().unwrap();
        std::env::set_current_dir(dir).unwrap();
        CwdGuard(original)
    }
}

impl Drop for CwdGuard {
    fn drop(&mut self) {
        let _ = std::env::set_current_dir(&self.0);
    }
}

fn env_guard(key: &'static str) {
    unsafe { std::env::remove_var(key) };
}

// ---------- resolve_local_defaults：纯函数，绝对路径，可并行 ----------

#[test]
fn probe_finds_repo_root_layout() {
    let t = TempDir::new("probe-repo");
    let dist = t.0.join("apps/mini-oc-web/web/dist");
    std::fs::create_dir_all(&dist).unwrap();
    std::fs::write(dist.join("index.html"), "<html></html>").unwrap();

    let (key, static_dir) = resolve_local_defaults(&t.0);
    assert_eq!(static_dir, dist, "应探测到仓库根布局的 dist");
    assert_eq!(key, t.0.join("data/cookie_key"));
}

#[test]
fn probe_prefers_dist_over_web_root() {
    let t = TempDir::new("probe-prefers");
    let dist = t.0.join("apps/mini-oc-web/web/dist");
    std::fs::create_dir_all(&dist).unwrap();
    std::fs::write(dist.join("index.html"), "dist").unwrap();
    // 旧布局：直接在 apps/mini-oc-web/web 下放 index.html
    let web_root = t.0.join("apps/mini-oc-web/web");
    std::fs::write(web_root.join("index.html"), "root").unwrap();

    let (_, static_dir) = resolve_local_defaults(&t.0);
    assert!(
        static_dir.ends_with("dist"),
        "同时存在时应优先 dist，实际 {:?}",
        static_dir
    );
}

#[test]
fn probe_falls_back_when_no_dist_found() {
    // 空目录（无任何 index.html）：回退 cwd/web/dist，不阻断启动
    let t = TempDir::new("probe-empty");
    let (key, static_dir) = resolve_local_defaults(&t.0);
    assert_eq!(static_dir, t.0.join("web/dist"));
    assert_eq!(key, t.0.join("data/cookie_key"));
}

// ---------- from_env 接线：涉及进程级 cwd / env，持锁串行 ----------

#[test]
fn from_env_local_defaults_boot_from_repo_root() {
    let _env = lock_env();
    let t = TempDir::new("fromenv-local");
    let dist = t.0.join("apps/mini-oc-web/web/dist");
    std::fs::create_dir_all(&dist).unwrap();
    std::fs::write(dist.join("index.html"), "<html></html>").unwrap();

    let _cwd = CwdGuard::enter(&t.0);
    env_guard("COOKIE_KEY_PATH");
    env_guard("WEB_STATIC_DIR");

    let state = AppState::from_env().expect("本地默认（无任何 env）应能直接启动");
    // macOS 下 current_dir 返回解析 symlink 后的路径（/var → /private/var），两边都 canonicalize 对齐
    assert_eq!(
        state.static_dir.canonicalize().unwrap(),
        dist.canonicalize().unwrap(),
        "未设 WEB_STATIC_DIR 应探测到 dist"
    );
    let key_file = t.0.join("data/cookie_key");
    assert!(key_file.is_file(), "key 应生成在 ./data/cookie_key");
    assert_eq!(state.cookie_key.len(), 32);
}

#[test]
fn from_env_explicit_vars_win_over_local_defaults() {
    let _env = lock_env();
    let t = TempDir::new("fromenv-explicit");
    let _cwd = CwdGuard::enter(&t.0);

    let explicit_key = t.0.join("explicit/cookie_key");
    std::fs::create_dir_all(t.0.join("explicit")).unwrap();
    std::env::set_var("COOKIE_KEY_PATH", explicit_key.to_str().unwrap());
    std::env::set_var("WEB_STATIC_DIR", "/explicit/static");

    let state = AppState::from_env().unwrap();
    assert_eq!(state.static_dir, PathBuf::from("/explicit/static"));
    assert!(explicit_key.is_file(), "key 应写在显式指定路径");
    assert!(
        !t.0.join("data/cookie_key").exists(),
        "显式指定时不应再生成默认路径的 key"
    );

    env_guard("COOKIE_KEY_PATH");
    env_guard("WEB_STATIC_DIR");
}
