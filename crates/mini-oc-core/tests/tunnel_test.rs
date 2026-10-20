//! Task 3：隧道地址拼装测试。

use mini_oc_core::tunnel::{remote_addr_for_rathole, tunnel_base_url, TunnelError};

#[test]
fn tunnel_base_url_table() {
    // (public_url, port, expected)
    let cases: Vec<(&str, u16, Result<String, TunnelError>)> = vec![
        // 无显式端口（https 单域名布局）→ 路径形态
        ("https://oc.example.com", 9464, Ok("https://oc.example.com/9464".into())),
        ("http://oc.example.com", 9001, Ok("http://oc.example.com/9001".into())),
        // 带显式端口（直连隧道端点）→ 原样
        ("http://1.2.3.4:9101", 9464, Ok("http://1.2.3.4:9101".into())),
        ("https://oc.example.com:8443", 9464, Ok("https://oc.example.com:8443".into())),
        // IPv6 host
        ("http://[::1]:8999", 9464, Ok("http://[::1]:8999".into())),
        ("http://[2001:db8::1]", 9001, Ok("http://[2001:db8::1]/9001".into())),
    ];
    for (public_url, port, expected) in cases {
        let got = tunnel_base_url(public_url, port);
        assert_eq!(got, expected, "tunnel_base_url({public_url}, {port})");
    }
}

#[test]
fn tunnel_base_url_rejects_malformed() {
    for bad in [
        "ftp://oc.example.com",   // 非 http(s)
        "https://oc.example.com/x", // 带路径
        "not a url",
        "",
        "https://oc.example.com/?q=1", // 带 query
    ] {
        assert!(
            tunnel_base_url(bad, 9464).is_err(),
            "{bad} 应被拒绝"
        );
    }
}

#[test]
fn remote_addr_for_rathole_table() {
    let cases: Vec<(&str, Result<String, TunnelError>)> = vec![
        // host + 固定 8999
        ("https://oc.example.com", Ok("oc.example.com:8999".into())),
        ("http://1.2.3.4:9101", Ok("1.2.3.4:9101".into())), // 带显式端口原样
        ("http://[::1]:9101", Ok("[::1]:9101".into())),
        ("https://oc.example.com:8443", Ok("oc.example.com:8443".into())),
    ];
    for (public_url, expected) in cases {
        let got = remote_addr_for_rathole(public_url);
        assert_eq!(got, expected, "remote_addr_for_rathole({public_url})");
    }
}

#[test]
fn remote_addr_rejects_malformed() {
    for bad in ["ftp://x.com", "https://a.com/path", "garbage"] {
        assert!(remote_addr_for_rathole(bad).is_err(), "{bad} 应被拒绝");
    }
}
