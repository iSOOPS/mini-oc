//! Task 4：opencode v2 API client 测试（wiremock 伪设备服务）。

use mini_oc_core::ocapi::{OcApi, OcClient, OcError};
use wiremock::matchers::{header, method, path, query_param, query_param_is_missing};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn client_for(server: &MockServer) -> OcClient {
    OcClient::new(&server.uri(), "testkey1234567890abcdefghij12345")
}

/// 附A 黄金样例（字段取自实测 /api/session 响应）。
const GOLDEN_SESSION: &str = r#"{
  "id": "ses_ec6dcc676ffe2puyLSwHkvf0yr",
  "projectID": "ac0cb1f40ceabf56e24a1f313b2e98ec84e86028",
  "agent": "build",
  "model": {"id": "GLM-5.3-Flash", "providerID": "GLM-YG", "variant": "default"},
  "cost": 0,
  "tokens": {"input": 309531, "output": 12016, "reasoning": 11245,
             "cache": {"read": 2014784, "write": 0}},
  "outcome": "succeeded",
  "time": {"created": 1791959972279, "updated": 1791970323934,
           "idle": 1791970323936, "viewed": 1791970323936},
  "title": "项目源码功能分析",
  "location": {"directory": "/Users/samuel/Documents/GitForAi/mini-oc-web"}
}"#;

#[tokio::test]
async fn info_ok_returns_version() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/info"))
        .and(header("Authorization", "Basic b3BlbmNvZGU6dGVzdGtleTEyMzQ1Njc4OTBhYmNkZWZnaGlqMTIzNDU="))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "version": "2.0.32", "pid": 52096,
                "urls": ["http://127.0.0.1:49878"],
                "paths": {"tmp": "/tmp/opencode"}
            })),
        )
        .mount(&server)
        .await;
    let info = client_for(&server).info().await.unwrap();
    assert_eq!(info.version, "2.0.32");
    assert_eq!(info.pid, 52096);
}

#[tokio::test]
async fn info_401_maps_unauthorized() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/info"))
        .respond_with(ResponseTemplate::new(401).set_body_json(
            serde_json::json!({"_tag": "UnauthorizedError", "message": "Authentication required"}),
        ))
        .mount(&server)
        .await;
    let err = client_for(&server).info().await.unwrap_err();
    assert!(matches!(err, OcError::Unauthorized(401)), "got {err:?}");
}

#[tokio::test]
async fn info_transport_error_on_dead_server() {
    // 端口 1 无监听 → 连接拒绝 → Transport
    let client = OcClient::new("http://127.0.0.1:1", "testkey1234567890abcdefghij12345");
    let err = client.info().await.unwrap_err();
    assert!(matches!(err, OcError::Transport), "got {err:?}");
}

#[tokio::test]
async fn projects_parses_array() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/project"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([
            {"id": "75a04d80", "canonical": "/Users/x/proj-a", "vcs": "git",
             "time": {"created": 1, "updated": 2, "active": 3}},
            {"id": "7888fc30", "canonical": "/Users/x/proj-b",
             "time": {"created": 1, "updated": 2, "active": 3}}
        ])))
        .mount(&server)
        .await;
    let projects = client_for(&server).projects().await.unwrap();
    assert_eq!(projects.len(), 2);
    assert_eq!(projects[0].id, "75a04d80");
    assert_eq!(projects[0].vcs.as_deref(), Some("git"));
    assert!(projects[1].vcs.is_none(), "宽容缺省字段");
}

#[tokio::test]
async fn sessions_follows_cursor_until_exhausted() {
    let server = MockServer::start().await;
    // 第一页：2 条 + next cursor
    Mock::given(method("GET"))
        .and(path("/api/session"))
        .and(query_param("limit", "50"))
        .and(query_param_is_missing("cursor"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "data": [
                {"id": "ses_1", "projectID": "p1", "cost": 0,
                 "tokens": {"input": 0, "output": 0, "reasoning": 0, "cache": {"read": 0, "write": 0}},
                 "time": {"created": 1, "updated": 2},
                 "location": {"directory": "/a"}},
                {"id": "ses_2", "projectID": "p1", "cost": 0,
                 "tokens": {"input": 0, "output": 0, "reasoning": 0, "cache": {"read": 0, "write": 0}},
                 "time": {"created": 1, "updated": 3},
                 "location": {"directory": "/a"}}
            ],
            "cursor": {"previous": null, "next": "cur-page-2"}
        })))
        .expect(1)
        .mount(&server)
        .await;
    // 第二页：1 条 + next=null
    Mock::given(method("GET"))
        .and(path("/api/session"))
        .and(query_param("cursor", "cur-page-2"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "data": [
                {"id": "ses_3", "projectID": "p1", "cost": 0,
                 "tokens": {"input": 0, "output": 0, "reasoning": 0, "cache": {"read": 0, "write": 0}},
                 "time": {"created": 1, "updated": 4},
                 "location": {"directory": "/a"}}
            ],
            "cursor": {"previous": "cur-page-2", "next": null}
        })))
        .expect(1)
        .mount(&server)
        .await;
    let sessions = client_for(&server).sessions(None).await.unwrap();
    assert_eq!(sessions.len(), 3);
    assert_eq!(sessions[2].id, "ses_3");
}

#[tokio::test]
async fn sessions_truncates_at_10_pages() {
    let server = MockServer::start().await;
    // 永远返回 next cursor（恶意/异常服务端）→ 最多 10 页即止，每页 1 条
    Mock::given(method("GET"))
        .and(path("/api/session"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "data": [
                {"id": "ses_x", "projectID": "p", "cost": 0,
                 "tokens": {"input": 0, "output": 0, "reasoning": 0, "cache": {"read": 0, "write": 0}},
                 "time": {"created": 1, "updated": 2},
                 "location": {"directory": "/a"}}
            ],
            "cursor": {"previous": null, "next": "always-more"}
        })))
        .mount(&server)
        .await;
    let sessions = client_for(&server).sessions(None).await.unwrap();
    assert_eq!(sessions.len(), 10, "10 页截断");
}

#[tokio::test]
async fn sessions_golden_sample_from_real_server() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/session"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "data": [serde_json::from_str::<serde_json::Value>(GOLDEN_SESSION).unwrap()],
            "cursor": {"previous": null, "next": null}
        })))
        .mount(&server)
        .await;
    let sessions = client_for(&server).sessions(None).await.unwrap();
    assert_eq!(sessions.len(), 1);
    let s = &sessions[0];
    assert_eq!(s.id, "ses_ec6dcc676ffe2puyLSwHkvf0yr");
    assert_eq!(s.project_id, "ac0cb1f40ceabf56e24a1f313b2e98ec84e86028");
    assert_eq!(s.title.as_deref(), Some("项目源码功能分析"));
    assert_eq!(s.directory, "/Users/samuel/Documents/GitForAi/mini-oc-web");
    assert_eq!(s.time.updated, 1791970323934);
}

#[tokio::test]
async fn pair_extracts_code() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/api/pair"))
        .respond_with(ResponseTemplate::new(200).set_body_json(
            serde_json::json!({"code": "610bsBT_9EqdC-VEYeb0tw", "expires_in": 300}),
        ))
        .mount(&server)
        .await;
    let code = client_for(&server).pair().await.unwrap();
    assert_eq!(code, "610bsBT_9EqdC-VEYeb0tw");
}

#[tokio::test]
async fn connect_exchanges_code_for_token() {
    // GET /auth/connect/{code} → {token}（公开端点，无需 basic auth；实测 curl 无 -u 亦成功）
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/auth/connect/610bsBT_9EqdC"))
        .respond_with(ResponseTemplate::new(200).set_body_json(
            serde_json::json!({"token": "1796025749.EKzkuAmKP9wfB4QAWNYZNKlPddvzWv9ruxIYnWJ4ZHg"}),
        ))
        .expect(1)
        .mount(&server)
        .await;
    let token = client_for(&server).connect("610bsBT_9EqdC").await.unwrap();
    assert_eq!(token, "1796025749.EKzkuAmKP9wfB4QAWNYZNKlPddvzWv9ruxIYnWJ4ZHg");
}

#[tokio::test]
async fn connect_invalid_code_maps_unauthorized() {
    // code 一次性：复用/伪造 → 401
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/auth/connect/bad"))
        .respond_with(ResponseTemplate::new(401).set_body_json(
            serde_json::json!({"_tag": "UnauthorizedError", "message": "Authentication required"}),
        ))
        .mount(&server)
        .await;
    let err = client_for(&server).connect("bad").await.unwrap_err();
    assert!(matches!(err, OcError::Unauthorized(401)), "got {err:?}");
}

#[tokio::test]
async fn requests_carry_basic_auth_header() {
    let server = MockServer::start().await;
    // base64("opencode:testkey1234567890abcdefghij12345")
    let expected = "Basic b3BlbmNvZGU6dGVzdGtleTEyMzQ1Njc4OTBhYmNkZWZnaGlqMTIzNDU=";
    Mock::given(method("GET"))
        .and(path("/api/info"))
        .and(header("Authorization", expected))
        .respond_with(ResponseTemplate::new(200).set_body_json(
            serde_json::json!({"version": "2.0.32", "pid": 1, "urls": [], "paths": {"tmp": "/t"}}),
        ))
        .expect(1)
        .mount(&server)
        .await;
    client_for(&server).info().await.unwrap();
}
