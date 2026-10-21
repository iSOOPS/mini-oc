//! Task 6：本地 serve 查询测试（wiremock 伪 serve）。

use mini_oc_app::api::{list_projects_at, list_sessions_at};
use wiremock::matchers::{method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

const KEY: &str = "k123456789012345678901234567890a";

#[tokio::test]
async fn projects_summarized_from_serve() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/project"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([
            {"id": "p1", "canonical": "/Users/x/proj-a", "vcs": "git",
             "time": {"created": 1, "updated": 2, "active": 3}},
            {"id": "p2", "canonical": "/Users/x/proj-b", "name": "自定义名",
             "time": {"created": 1, "updated": 2, "active": 3}}
        ])))
        .mount(&server)
        .await;
    let port = server.uri().rsplit(':').next().unwrap().parse::<u16>().unwrap();
    let projects = list_projects_at(port, KEY).await.unwrap();
    assert_eq!(projects.len(), 2);
    assert_eq!(projects[0].name, "proj-a", "无 name 时取路径末段");
    assert_eq!(projects[0].path, "/Users/x/proj-a");
    assert_eq!(projects[0].vcs.as_deref(), Some("git"));
    assert_eq!(projects[1].name, "自定义名", "优先用服务端 name");
    assert!(projects[1].vcs.is_none());
}

#[tokio::test]
async fn sessions_summarized_with_directory_filter() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/session"))
        .and(query_param("directory", "/Users/x/proj-a"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "data": [
                {"id": "ses_1", "projectID": "p1", "cost": 0,
                 "tokens": {"input": 0, "output": 0, "reasoning": 0, "cache": {"read": 0, "write": 0}},
                 "time": {"created": 1, "updated": 1791970323934i64},
                 "title": "会话一", "location": {"directory": "/Users/x/proj-a"}}
            ],
            "cursor": {"previous": null, "next": null}
        })))
        .expect(1)
        .mount(&server)
        .await;
    let port = server.uri().rsplit(':').next().unwrap().parse::<u16>().unwrap();
    let sessions = list_sessions_at(port, KEY, Some("/Users/x/proj-a".into())).await.unwrap();
    assert_eq!(sessions.len(), 1);
    assert_eq!(sessions[0].id, "ses_1");
    assert_eq!(sessions[0].title.as_deref(), Some("会话一"));
    assert_eq!(sessions[0].updated_ms, 1791970323934);
    server.verify().await;
}

#[tokio::test]
async fn query_serve_down_returns_clear_error() {
    // 端口无服务
    let err = list_projects_at(1, KEY).await.unwrap_err();
    assert!(err.contains("不可达") || err.contains("连接") || err.contains("Transport"), "错误信息: {err}");
}
