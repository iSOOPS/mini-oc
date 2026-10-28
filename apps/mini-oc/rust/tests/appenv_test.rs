//! appenv 模块测试：`.env` 文件持久化（SERVER_URL / REMEMBER_PASSWORD / PASSWORD）。
//!
//! 注意：MINI_OC_HOME 是进程级 env——所有测试经全局 Mutex 串行，避免并行竞态。

use std::path::PathBuf;
use std::sync::Mutex;
use std::sync::OnceLock;

fn test_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

/// 进程级 env 隔离测试目录，返回原值以便恢复。
fn isolate_home(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("mini-oc-appenv-test-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::env::set_var("MINI_OC_HOME", &dir);
    dir
}

fn restore_home() {
    std::env::remove_var("MINI_OC_HOME");
    std::env::remove_var("MINI_OC_SERVER_URL");
    std::env::remove_var("MINI_OC_REMEMBER_PASSWORD");
    std::env::remove_var("MINI_OC_PASSWORD");
}

#[test]
fn env_file_roundtrip() {
    let _g = test_lock().lock().unwrap();
    let dir = isolate_home("roundtrip");

    // 初始：文件不存在 → 空配置
    let cfg = rust_lib_mini_oc::api::appenv::load_app_env();
    assert_eq!(cfg.server_url, "", "文件缺失时 server_url 为空");
    assert!(!cfg.remember_password, "文件缺失时 remember 为 false");
    assert_eq!(cfg.password, "", "文件缺失时 password 为空");

    // 写入 → 读取回环
    rust_lib_mini_oc::api::appenv::save_app_env(&rust_lib_mini_oc::api::appenv::AppEnv {
        server_url: "https://oc.isoops.com".into(),
        remember_password: true,
        password: "secret-key-123".into(),
    })
    .expect("save");

    let cfg = rust_lib_mini_oc::api::appenv::load_app_env();
    assert_eq!(cfg.server_url, "https://oc.isoops.com");
    assert!(cfg.remember_password);
    assert_eq!(cfg.password, "secret-key-123");

    // 0600 权限（unix）
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let meta = std::fs::metadata(dir.join(".env")).unwrap();
        assert_eq!(meta.permissions().mode() & 0o777, 0o600, "env 文件权限 0600");
    }

    // 删除密码（记住密码取消）：password 空、remember false，但 server_url 保留
    rust_lib_mini_oc::api::appenv::save_app_env(&rust_lib_mini_oc::api::appenv::AppEnv {
        server_url: "https://oc.isoops.com".into(),
        remember_password: false,
        password: "".into(),
    })
    .expect("save");
    let cfg = rust_lib_mini_oc::api::appenv::load_app_env();
    assert_eq!(cfg.server_url, "https://oc.isoops.com", "地址保留");
    assert!(!cfg.remember_password);
    assert_eq!(cfg.password, "", "密码已删除");

    let _ = std::fs::remove_dir_all(&dir);
    restore_home();
}

#[test]
fn env_file_preserves_unknown_keys() {
    let _g = test_lock().lock().unwrap();
    let dir = isolate_home("preserve");

    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join(".env"),
        "# comment\nFOO_BAR=hello\nSERVER_URL=https://old.example.com\n",
    )
    .unwrap();

    // 只更新受管 key，其他行保留
    rust_lib_mini_oc::api::appenv::save_app_env(&rust_lib_mini_oc::api::appenv::AppEnv {
        server_url: "https://new.example.com".into(),
        remember_password: true,
        password: "p@ss".into(),
    })
    .expect("save");

    let raw = std::fs::read_to_string(dir.join(".env")).unwrap();
    assert!(raw.contains("FOO_BAR=hello"), "未知 key 保留");
    assert!(raw.contains("# comment"), "注释保留");
    assert!(raw.contains("SERVER_URL=https://new.example.com"), "受管 key 就地更新");
    assert!(!raw.contains("old.example.com"), "旧值被替换");

    let _ = std::fs::remove_dir_all(&dir);
    restore_home();
}

#[test]
fn docker_env_vars_override_file() {
    let _g = test_lock().lock().unwrap();
    let dir = isolate_home("docker");

    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join(".env"),
        "SERVER_URL=https://file.example.com\nREMEMBER_PASSWORD=true\nPASSWORD=from-file\n",
    )
    .unwrap();

    // docker 逻辑：显式进程 env 永远优先于文件
    std::env::set_var("MINI_OC_SERVER_URL", "https://docker.example.com");
    std::env::set_var("MINI_OC_REMEMBER_PASSWORD", "false");
    std::env::set_var("MINI_OC_PASSWORD", "from-docker");

    let cfg = rust_lib_mini_oc::api::appenv::load_app_env();
    assert_eq!(cfg.server_url, "https://docker.example.com");
    assert!(!cfg.remember_password);
    assert_eq!(cfg.password, "from-docker");

    // 注入 env 时写操作不落文件（容器文件系统只读场景友好）
    rust_lib_mini_oc::api::appenv::save_app_env(&rust_lib_mini_oc::api::appenv::AppEnv {
        server_url: "https://try-write.example.com".into(),
        remember_password: false,
        password: String::new(),
    })
    .expect("save");
    let raw = std::fs::read_to_string(dir.join(".env")).unwrap();
    assert!(
        raw.contains("https://file.example.com"),
        "注入 env 时写操作不覆盖文件"
    );

    let _ = std::fs::remove_dir_all(&dir);
    restore_home();
}

#[test]
fn server_url_effective_with_default() {
    let _g = test_lock().lock().unwrap();
    let dir = isolate_home("effective");

    // 无文件无 env → 默认地址
    assert_eq!(
        rust_lib_mini_oc::api::appenv::server_url_effective(),
        "https://oc.isoops.com",
        "默认地址兜底"
    );

    // 文件有值 → 文件值
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join(".env"), "SERVER_URL=https://custom.example.com\n").unwrap();
    assert_eq!(
        rust_lib_mini_oc::api::appenv::server_url_effective(),
        "https://custom.example.com"
    );

    let _ = std::fs::remove_dir_all(&dir);
    restore_home();
}

#[test]
fn server_url_effective_falls_back_to_config() {
    let _g = test_lock().lock().unwrap();
    let dir = isolate_home("fallback-config");

    // .env 无地址，但老用户 config.json 已有 → 用 config 值（升级迁移）
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("config.json"),
        r#"{"server_url":"https://legacy.example.com","port":9464,"theme":"system"}"#,
    )
    .unwrap();
    assert_eq!(
        rust_lib_mini_oc::api::appenv::server_url_effective(),
        "https://legacy.example.com",
        "config.json 兜底"
    );

    let _ = std::fs::remove_dir_all(&dir);
    restore_home();
}
