//! Task 12：本地端到端链路（真实 opencode serve）。
//!
//! 前置：PATH 上有 opencode。CI/无 opencode 环境自动跳过。
//! 覆盖：start_local（真进程+真探针）→ list_projects/list_sessions（真 API）→ 终端计划生成。

use mini_oc_app::api::{list_projects_at, list_sessions_at, resolve_opencode_bin};
use mini_oc_app::supervisor::{SupConfig, Supervisor};

fn opencode_available() -> bool {
    std::process::Command::new("opencode")
        .arg("--version")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// 真实 opencode serve 的 Supervisor（与 api 层同款命令构造）。
fn real_supervisor(key: &str) -> Supervisor {
    let serve_key = key.to_string();
    let probe_key = key.to_string();
    Supervisor::new(SupConfig {
        serve_command: Box::new(move |port| {
            let mut cmd = std::process::Command::new(resolve_opencode_bin());
            cmd.arg("serve").arg("--port").arg(port.to_string());
            cmd.env("OPENCODE_SERVER_PASSWORD", &serve_key);
            cmd
        }),
        rathole_command: Box::new(|_| {
            let mut c = std::process::Command::new("/bin/sleep");
            c.arg("300");
            c
        }),
        probe: Box::new(move |port| {
            let client =
                mini_oc_core::ocapi::OcClient::new(&format!("http://127.0.0.1:{port}"), &probe_key);
            Box::pin(async move {
                use mini_oc_core::ocapi::OcApi;
                client.info().await.map(|i| i.version).map_err(|e| e.to_string())
            })
        }),
    })
}

fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

const KEY: &str = "e2etestkey1234567890abcdefghijklmnop";

#[tokio::test]
#[ignore = "需要本机 opencode（e2e）"]
async fn e2e_local_serve_projects_and_terminal_plan() {
    if !opencode_available() {
        eprintln!("opencode 不可用，跳过");
        return;
    }
    let sup = real_supervisor(KEY);
    let port = free_port();
    sup.start_local(port).await.expect("真实 serve 启动");

    // 项目列表（真 API；本机至少有当前 mini-oc 仓库项目）
    let projects = list_projects_at(port, KEY).await.expect("项目列表");
    assert!(!projects.is_empty(), "本机应有至少一个 opencode 项目");
    eprintln!("项目数：{}", projects.len());

    // 任一项目的会话列表（真 API，可为空但不报错）
    let sessions = list_sessions_at(port, KEY, Some(projects[0].path.clone())).await;
    assert!(sessions.is_ok(), "会话查询不报错: {:?}", sessions.err());

    // 终端启动计划（凭据零泄露）
    let dir = tempfile::tempdir().unwrap();
    let plan = mini_oc_app::term_spawn::build_terminal_plan(
        &resolve_opencode_bin(),
        &format!("http://127.0.0.1:{port}"),
        &projects[0].path,
        None,
        Some(dir.path()),
        KEY,
    )
    .unwrap();
    assert!(!plan.display_command.contains(KEY));

    sup.stop_all().await.unwrap();
    // 无孤儿进程
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    let st = sup.state();
    assert_eq!(st.mode_str(), "off");
}
