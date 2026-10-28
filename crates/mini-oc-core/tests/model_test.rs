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
fn migrate_v1_rejects_garbage() {
    assert!(migrate_v1("not json at all").is_err());
}

/// 真实 V1 输出形态（kebab-case 字段 + cloud_ip + sb 块 + oc-port）。
/// 样例格式与 V1 mini-oc-web users.rs 的 serde 定义逐字段对齐。
fn v1_real_fixture() -> String {
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
                        "oc-port": 9464,
                        "device-name": "Samuels-MacBook",
                        "pctype": "macos",
                        "bound": true,
                        "public-url": "https://oc.example.com"
                    }
                ],
                "sb": {"base_url": "http://sb:3000", "username": "u", "password": "p"},
                "created_at": "2026-09-01T00:00:00Z",
                "updated_at": "2026-09-02T00:00:00Z"
            },
            {
                "id": "9f8b7c6d-1111-2222-3333-444455556666",
                "name": "legacy-uuid-user",
                "key": "k123456789012345678901234567890b",
                "cloud_ip": "",
                "last_used_at": null,
                "devices": [
                    "bare-string-device",
                    {"d-name": "老格式设备", "name": "old-dev", "pctype": "windows", "public-url": "http://1.2.3.4:9101"}
                ],
                "sb": {"base_url": "http://sb:3000", "username": "u", "password": "p"},
                "created_at": "2026-08-01T00:00:00Z",
                "updated_at": "2026-08-01T00:00:00Z"
            }
        ]
    })
    .to_string()
}

#[test]
fn migrate_real_v1_kebab_case_fixture() {
    // C1 回归：V1 设备字段是 kebab-case（oc-port/public-url/device-name），
    // 早期实现误按 snake_case 解析导致真实数据整表失败。
    let f = migrate_v1(&v1_real_fixture()).unwrap();
    assert_eq!(f.version, 2);
    assert_eq!(f.users.len(), 2, "两个用户都保留");

    // 用户 1：oc-port → port 语义迁移
    let u1 = &f.users[0];
    assert_eq!(u1.id, "100001");
    assert_eq!(u1.devices.len(), 1);
    let d = &u1.devices[0];
    assert_eq!(d.port, 9464, "port 取 V1 oc-port");
    assert_eq!(d.public_url, "https://oc.example.com", "public-url 正确读取");
    assert_eq!(d.device_name, "Samuels-MacBook", "device-name 正确读取");
    assert_eq!(d.pctype, "macos");

    // 用户 2：uuid id 改写为 6 位数字（保留用户，不丢弃）
    let u2 = &f.users[1];
    assert_eq!(u2.id.len(), 6);
    assert!(u2.id.chars().all(|c| c.is_ascii_digit()), "uuid 改写为 6 位数字");
    assert_ne!(u1.id, u2.id, "改写后的 id 不冲突");
    // 裸字符串设备跳过（desc/pctype/public_url 全空，本就不可用）；d-name 别名设备保留
    assert_eq!(u2.devices.len(), 1, "仅保留完整设备");
    assert_eq!(u2.devices[0].name, "old-dev");
    assert_eq!(u2.devices[0].desc, "老格式设备", "d-name 别名生效");
    assert_eq!(u2.devices[0].port, 4040, "缺省 port 取 legacy 默认 4040");
}

#[test]
fn migrate_kebab_and_snake_aliases_both_accepted() {
    // V2 内部已写过 snake_case（本仓库早期误写）→ 双形态兼容读取
    let snake = r#"{"version":1,"users":[{"id":"100002","name":"u2","key":"k123456789012345678901234567890a","created_at":"t","updated_at":"t","devices":[{"desc":"d","name":"pc-1","port":9001,"oc_port":9009,"device_name":"PC","pctype":"windows","public_url":"http://1.2.3.4:9101"}]}]}"#;
    let f = migrate_v1(snake).unwrap();
    assert_eq!(f.users[0].devices[0].port, 9009, "snake_case oc_port 别名");
    assert_eq!(f.users[0].devices[0].public_url, "http://1.2.3.4:9101");
    assert_eq!(f.users[0].devices[0].device_name, "PC");
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
        enabled: true,
        device_id: String::new(),
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
        pending_devices: vec![],
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

#[test]
fn old_json_keeps_existing_devices_enabled() {
    // 旧格式 JSON：无 enabled/device_id/pending_devices 字段
    let raw = r#"{"version":2,"users":[{"id":"100001","name":"samuel","key":"testkey1234567890abcdefghij12345","devices":[{"desc":"工作机","name":"macbook-pro","port":9464,"pctype":"macos","publicUrl":"https://t.example.com"}],"createdAt":"2026-01-01T00:00:00Z","updatedAt":"2026-01-01T00:00:00Z"}]}"#;
    let f: UsersFile = serde_json::from_str(raw).unwrap();
    let d = &f.users[0].devices[0];
    assert!(d.enabled, "缺省 enabled 必须为 true");
    assert_eq!(d.device_id, "");
    assert!(f.users[0].pending_devices.is_empty());
}

#[test]
fn pending_device_validate() {
    let mut p = PendingDevice { device_id: generate_device_id(), desc: String::new(), port: 9464, pctype: "macos".into(), device_name: "Mac".into(), reported_at: "2026-10-28T00:00:00Z".into() };
    assert!(p.validate().is_ok());
    p.pctype = "linux".into();
    assert!(p.validate().is_err());
    p.pctype = "macos".into();
    p.port = 0;
    assert!(p.validate().is_err());
}

#[test]
fn generate_device_id_is_uuid_v4() {
    let id = generate_device_id();
    assert_eq!(id.len(), 36);
    assert_eq!(char::from(id.as_bytes()[14]), '4'); // version nibble
    let c = char::from(id.as_bytes()[19]); // variant nibble
    assert!(c == '8' || c == '9' || c == 'a' || c == 'b');
}
