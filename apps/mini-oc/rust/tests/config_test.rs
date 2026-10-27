//! Task 2：本地配置持久化测试。

use rust_lib_mini_oc::config::{load_config_from, save_config_to, ClientConfig, StoredUser};

fn sample() -> ClientConfig {
    ClientConfig {
        server_url: "https://oc.example.com".into(),
        user: Some(StoredUser {
            id: "100001".into(),
            name: "samuel".into(),
            key: "k".repeat(32),
        }),
        bound_device: Some("macbook-pro".into()),
        port: 9464,
        opencode_bin: None,
        theme: "system".into(),
    }
}

#[test]
fn config_roundtrip_preserves_all_fields() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.json");
    save_config_to(&path, &sample()).unwrap();
    let loaded = load_config_from(&path);
    assert_eq!(loaded.server_url, "https://oc.example.com");
    let user = loaded.user.unwrap();
    assert_eq!(user.id, "100001");
    assert_eq!(user.key.len(), 32);
    assert_eq!(loaded.bound_device.as_deref(), Some("macbook-pro"));
    assert_eq!(loaded.port, 9464);
    assert_eq!(loaded.theme, "system");
}

#[test]
fn config_missing_file_returns_default() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("nonexistent.json");
    let cfg = load_config_from(&path);
    assert_eq!(cfg.server_url, "");
    assert!(cfg.user.is_none());
    assert_eq!(cfg.port, 9464, "默认端口");
    assert_eq!(cfg.theme, "system");
}

#[test]
fn config_corrupted_file_returns_default_not_panic() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.json");
    std::fs::write(&path, "{ not json").unwrap();
    let cfg = load_config_from(&path);
    assert!(cfg.user.is_none(), "损坏文件不炸，回退默认");
}

#[test]
fn config_file_created_with_owner_only_permission() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.json");
    save_config_to(&path, &sample()).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&path).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600, "凭据文件权限必须 0600");
    }
}
