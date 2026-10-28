//! 集成测试公共环境：wiremock SB + 随机端口 axum 实例。

use mini_oc_core::device::{PortalUser, UserDevice, UsersFile};
use mini_oc_web::state::AppState;
use std::sync::Arc;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

/// 测试专用固定 cookie 密钥。
pub const TEST_COOKIE_KEY: [u8; 32] = [7u8; 32];

/// 启动应用（SB 指向 wiremock 伪服务）。
pub async fn spawn_app(sb: &MockServer) -> String {
    spawn_app_with_static(sb, None).await
}

/// 启动应用（可指定 SPA 静态目录，避免测试间 env 竞态）。
pub async fn spawn_app_with_static(sb: &MockServer, static_dir: Option<String>) -> String {
    if let Some(dir) = static_dir {
        std::env::set_var("WEB_STATIC_DIR", dir);
    }
    let state = AppState::with_key(&sb.uri(), "admin", "sbpass", TEST_COOKIE_KEY.to_vec()).unwrap();
    let app = mini_oc_web::routes::build_router(Arc::new(state));
    std::env::remove_var("WEB_STATIC_DIR");
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    format!("http://{addr}")
}

/// 在 wiremock 上挂 SB 登录 + 注册表读取。
pub async fn mount_sb_users(sb: &MockServer, body: &str) {
    Mock::given(method("POST"))
        .and(path("/.auth"))
        .respond_with(ResponseTemplate::new(200).append_header("set-cookie", "auth_127_0_0_1=jwt-token; Path=/"))
        .named("sb-login")
        .mount(sb)
        .await;
    Mock::given(method("GET"))
        .and(path("/.fs/web/opencode/config.md"))
        .respond_with(ResponseTemplate::new(200).set_body_string(body))
        .named("sb-users-get")
        .mount(sb)
        .await;
}

/// 构造测试用户（单设备）。
pub fn test_user() -> PortalUser {
    PortalUser {
        id: "100001".into(),
        name: "samuel".into(),
        key: "testkey1234567890abcdefghij12345".into(),
        devices: vec![UserDevice {
            desc: "工作电脑".into(),
            name: "macbook-pro".into(),
            port: 9464,
            pctype: "macos".into(),
            device_name: "Samuels-MacBook".into(),
            bound: true,
            public_url: "https://oc.example.com".into(),
            enabled: true,
            device_id: String::new(),
        }],
        pending_devices: vec![],
        created_at: "2026-09-01T00:00:00Z".into(),
        updated_at: "2026-09-01T00:00:00Z".into(),
        last_used_at: None,
    }
}

pub fn users_json(users: Vec<PortalUser>) -> String {
    serde_json::to_string(&UsersFile { version: 2, users }).unwrap()
}
