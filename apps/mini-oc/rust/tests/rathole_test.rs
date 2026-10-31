//! Task 3：rathole 配置生成与二进制定位测试。

use rust_lib_mini_oc::account::DeviceBrief;
use rust_lib_mini_oc::rathole::{rathole_toml, resolve_rathole_bin};

fn device(public_url: &str) -> DeviceBrief {
    DeviceBrief {
        name: "macbook-pro".into(),
        port: 9464,
        pctype: "macos".into(),
        bound: true,
        device_name: "Samuels-MacBook".into(),
        public_url: public_url.into(),
        device_id: String::new(),
        enabled: true,
    }
}

#[test]
fn toml_single_service_with_domain_public_url() {
    // 无显式端口形态：remote_addr = host:8999
    let toml = rathole_toml(&device("https://oc.example.com"), "userkey1234567890abcdef").unwrap();
    let val: toml::Value = toml::from_str(&toml).unwrap();
    assert_eq!(val["client"]["remote_addr"].as_str(), Some("oc.example.com:8999"));
    let svc = &val["client"]["services"]["macbook-pro"];
    assert_eq!(svc["token"].as_str(), Some("userkey1234567890abcdef"));
    assert_eq!(svc["local_addr"].as_str(), Some("127.0.0.1:9464"));
    // 单 service 契约：不得出现第二个 service
    let services = val["client"]["services"].as_table().unwrap();
    assert_eq!(services.len(), 1, "V2 只有单 service：{services:?}");
}

#[test]
fn toml_single_service_with_explicit_port() {
    // 带显式端口形态：remote_addr 原样 host:port
    let toml = rathole_toml(&device("http://1.2.3.4:9101"), "k").unwrap();
    let val: toml::Value = toml::from_str(&toml).unwrap();
    assert_eq!(val["client"]["remote_addr"].as_str(), Some("1.2.3.4:9101"));
}

#[test]
fn toml_rejects_invalid_public_url() {
    assert!(rathole_toml(&device("not-a-url"), "k").is_err());
}

#[test]
fn rathole_bin_env_override() {
    // RATHOLE_BIN env 优先
    std::env::set_var("RATHOLE_BIN", "/tmp/fake-rathole");
    let bin = resolve_rathole_bin();
    assert_eq!(bin.as_ref().map(|p| p.to_string_lossy().to_string()).as_deref(), Some("/tmp/fake-rathole"));
    std::env::remove_var("RATHOLE_BIN");
    // 无 env 时回退 assets/PATH 搜索（本机无 bundle → None 或 PATH 命中）
    let _ = resolve_rathole_bin();
}

/// I17：macOS .app bundle 布局的 rathole 探测。
///
/// 症状：打包后的 .app 运行报「rathole 客户端不存在」。
/// 根因：current_exe() 在 .app 内是 Contents/MacOS/mini_oc，而 build 脚本
/// 打包 rathole 到 **Contents/Resources/assets/**（兄弟目录）——旧查找
/// 「exe 同目录 assets」永远落空。
///
/// 修复：新增 bundle 布局探测（MacOS/../Resources/assets/...），
/// 经 `resolve_rathole_bin_near(exe_dir)` 可测试入口验证（不依赖真实安装）。
#[cfg(target_os = "macos")]
#[test]
fn resolve_finds_rathole_in_macos_app_bundle_layout() {
    use rust_lib_mini_oc::rathole::resolve_rathole_bin_near;

    // 模拟 .app 结构（exe 在 Contents/MacOS/，rathole 在 Contents/Resources/assets/）
    let tmp = tempfile::tempdir().unwrap();
    let exe_dir = tmp.path().join("Contents/MacOS");
    std::fs::create_dir_all(&exe_dir).unwrap();
    let plat = format!("macos-{}", std::env::consts::ARCH);
    let bin = tmp
        .path()
        .join("Contents/Resources/assets/rathole/bin")
        .join(&plat)
        .join("rathole");
    std::fs::create_dir_all(bin.parent().unwrap()).unwrap();
    std::fs::write(&bin, b"fake").unwrap();

    let found = resolve_rathole_bin_near(&exe_dir, &plat);
    assert_eq!(found, Some(bin), "应探测到 Resources 下的 bundle rathole");
}

#[cfg(target_os = "macos")]
#[test]
fn resolve_near_returns_none_when_no_bundle_anywhere() {
    use rust_lib_mini_oc::rathole::resolve_rathole_bin_near;

    let tmp = tempfile::tempdir().unwrap();
    let exe_dir = tmp.path().join("Contents/MacOS");
    std::fs::create_dir_all(&exe_dir).unwrap();

    // 空目录树：无 bundle → None（PATH 回退由调用方处理，此处测纯路径探测）
    let found = resolve_rathole_bin_near(&exe_dir, "macos-aarch64");
    assert_eq!(found, None, "无 bundle 时路径探测应返回 None");
}
