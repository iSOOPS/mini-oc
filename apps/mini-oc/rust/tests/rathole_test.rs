//! Task 3：rathole 配置生成与二进制定位测试。

use mini_oc_app::account::DeviceBrief;
use mini_oc_app::rathole::{rathole_toml, resolve_rathole_bin};

fn device(public_url: &str) -> DeviceBrief {
    DeviceBrief {
        name: "macbook-pro".into(),
        port: 9464,
        pctype: "macos".into(),
        bound: true,
        device_name: "Samuels-MacBook".into(),
        public_url: public_url.into(),
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
