//! Task 2：用户/设备模型 + V1→V2 迁移测试。

use mini_oc_core::device::*;

/// V1 注册表样例：snake_case 字段（V1 serde 默认），含 oc_port / cloud_ip / sb 等 V2 已废弃字段。
fn v1_fixture() -> String {
    serde_json::json!({
        "version": 1,
        "users": [
            {
                "id": "100001",
                "name": "samuel",
                "key": "k123456789012345678901234567890a",
                "cloud_ip": "http://1.2.3.4:8100",
                "last_used_at": "2026-10-01T00:00:00Z",
                "devices": [
                    {
                        "desc": "工作电脑",
                        "name": "macbook-pro",
                        "port": 9465,
                        "oc_port": 9464,
                        "device_name": "Samuels-MacBook",
                        "pctype": "macos",
                        "bound": true,
                        "public_url": "https://oc.example.com"
                    }
                ],
                "sb": {"base_url": "http://sb:3000", "username": "u", "password": "p"},
                "created_at": "2026-09-01T00:00:00Z",
                "updated_at": "2026-09-02T00:00:00Z"
            }
        ]
    })
    .to_string()
}

#[test]
fn migrate_maps_oc_port_to_port() {
    // V2 的 port 语义 = opencode serve 端口 = V1 的 oc_port
    let f = migrate_v1(&v1_fixture()).unwrap();
    assert_eq!(f.version, 2);
    assert_eq!(f.users[0].devices[0].port, 9464);
}

#[test]
fn migrate_keeps_port_when_oc_port_absent() {
    let raw = r#"{"version":1,"users":[{"id":"100002","name":"u2","key":"k123456789012345678901234567890a","created_at":"t","updated_at":"t","devices":[{"desc":"d","name":"pc-1","port":9001,"pctype":"windows","device_name":"","bound":false,"public_url":"http://1.2.3.4:9101"}]}]}"#;
    let f = migrate_v1(raw).unwrap();
    assert_eq!(f.users[0].devices[0].port, 9001);
}

#[test]
fn migrate_strips_legacy_fields() {
    let f = migrate_v1(&v1_fixture()).unwrap();
    let json = serde_json::to_value(&f).unwrap();
    // 序列化输出不得含任何废弃字段
    assert!(json["users"][0].get("cloudIp").is_none());
    assert!(json["users"][0].get("sb").is_none());
    assert!(json["users"][0]["devices"][0].get("ocPort").is_none());
    // 保留字段精确断言
    assert_eq!(json["users"][0]["id"], "100001");
    assert_eq!(json["users"][0]["name"], "samuel");
    assert_eq!(json["users"][0]["key"], "k123456789012345678901234567890a");
    assert_eq!(json["users"][0]["devices"][0]["name"], "macbook-pro");
    assert_eq!(json["users"][0]["devices"][0]["pctype"], "macos");
    assert_eq!(json["users"][0]["devices"][0]["publicUrl"], "https://oc.example.com");
    assert_eq!(json["users"][0]["devices"][0]["bound"], true);
}

#[test]
fn migrate_skips_broken_device_keeps_rest() {
    let raw = r#"{"version":1,"users":[{"id":"100003","name":"u3","key":"k123456789012345678901234567890a","created_at":"t","updated_at":"t","devices":[
        {"desc":"d","name":"BAD NAME!!","port":9001,"pctype":"windows","device_name":"","bound":false,"public_url":"http://1.2.3.4:9101"},
        {"desc":"d2","name":"good-pc","port":9002,"pctype":"macos","device_name":"","bound":false,"public_url":"https://oc.example.com"}
    ]}]}"#;
    let f = migrate_v1(raw).unwrap();
    assert_eq!(f.users[0].devices.len(), 1);
    assert_eq!(f.users[0].devices[0].name, "good-pc");
}

#[test]
fn migrate_accepts_missing_version_field() {
    // 更旧的格式：无 version 字段，按 V1 处理
    let raw = r#"{"users":[{"id":"100004","name":"u4","key":"k123456789012345678901234567890a","created_at":"t","updated_at":"t","devices":[]}]}"#;
    let f = migrate_v1(raw).unwrap();
    assert_eq!(f.version, 2);
    assert_eq!(f.users.len(), 1);
}

#[test]
fn migrate_rejects_garbage() {
    assert!(migrate_v1("not json at all").is_err());
}

#[test]
fn validate_rules() {
    let good = UserDevice {
        desc: "工作电脑".into(),
        name: "macbook-pro".into(),
        port: 9464,
        pctype: "macos".into(),
        device_name: "Samuels-MacBook".into(),
        bound: true,
        public_url: "https://oc.example.com".into(),
    };
    assert!(good.validate().is_ok());

    for (field, bad) in [
        ("name 大写", UserDevice { name: "BadName".into(), ..good.clone() }),
        ("name 非法字符", UserDevice { name: "bad_name!".into(), ..good.clone() }),
        ("name 空", UserDevice { name: "".into(), ..good.clone() }),
        ("pctype 非法", UserDevice { pctype: "linux".into(), ..good.clone() }),
        ("public_url 带路径", UserDevice { public_url: "https://oc.example.com/x".into(), ..good.clone() }),
        ("public_url 非 http", UserDevice { public_url: "ftp://oc.example.com".into(), ..good.clone() }),
        ("desc 超长", UserDevice { desc: "x".repeat(65), ..good.clone() }),
        ("desc 空", UserDevice { desc: "".into(), ..good.clone() }),
    ] {
        assert!(bad.validate().is_err(), "{field} 应被拒绝");
    }
    // public_url 带显式端口合法
    let with_port = UserDevice { public_url: "http://1.2.3.4:9101".into(), ..good };
    assert!(with_port.validate().is_ok());
}

#[test]
fn user_validate_rules() {
    let good = PortalUser {
        id: "100001".into(),
        name: "samuel".into(),
        key: "k123456789012345678901234567890a".into(),
        devices: vec![],
        created_at: "2026-09-01T00:00:00Z".into(),
        updated_at: "2026-09-01T00:00:00Z".into(),
        last_used_at: None,
    };
    assert!(good.validate().is_ok());
    assert!(PortalUser { id: "12345".into(), ..good.clone() }.validate().is_err(), "id 必须 6 位");
    assert!(PortalUser { id: "12345a".into(), ..good.clone() }.validate().is_err(), "id 必须数字");
    assert!(PortalUser { name: "非法 名".into(), ..good.clone() }.validate().is_err(), "name 白名单");
    assert!(PortalUser { key: "short".into(), ..good.clone() }.validate().is_err(), "key 32 位");
}

#[test]
fn generate_user_id_format_and_unique() {
    let existing: Vec<String> = (0..50).map(|i| format!("{:06}", 990000 + i)).collect();
    for _ in 0..10 {
        let id = generate_user_id(&existing);
        assert_eq!(id.len(), 6);
        assert!(id.chars().all(|c| c.is_ascii_digit()));
        assert!(!existing.contains(&id), "生成的 id 不得与现有冲突");
    }
}

#[test]
fn generate_key_format() {
    let key = generate_key();
    assert_eq!(key.len(), 32);
    assert!(key.chars().all(|c| c.is_ascii_alphanumeric()));
    assert_ne!(generate_key(), generate_key(), "两次生成不得相同");
}
