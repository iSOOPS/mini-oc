//! Task 5：环形日志缓冲测试。
//!
//! LogBuffer 是全局单例（生产语义）——测试经全局 Mutex 串行避免串扰。

use rust_lib_mini_oc::logbuf::{push_log, recent_logs, clear_logs, subscribe_logs_channel, flush_now};
use std::sync::{Mutex, MutexGuard};
use std::time::Duration;

static ENV_LOCK: Mutex<()> = Mutex::new(());

fn setup() -> MutexGuard<'static, ()> {
    let guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    clear_logs();
    guard
}

#[test]
fn ring_capacity_truncates_at_2000() {
    let _guard = setup();
    for i in 0..2500 {
        push_log("opencode", "info", &format!("line-{i}"));
    }
    let logs = recent_logs(usize::MAX);
    assert_eq!(logs.len(), 2000, "环形上限 2000");
    assert!(logs[0].message.contains("line-500"), "最旧保留的是第 500 条: {}", logs[0].message);
    assert!(logs[1999].message.contains("line-2499"), "最新是第 2499 条");
}

#[test]
fn source_and_level_marked() {
    let _guard = setup();
    push_log("rathole", "warn", "连接失败");
    push_log("system", "info", "服务启动");
    let logs = recent_logs(10);
    assert_eq!(logs[0].source, "rathole");
    assert_eq!(logs[0].level, "warn");
    assert_eq!(logs[1].source, "system");
    // 时间戳单调
    assert!(logs[1].ts_ms >= logs[0].ts_ms);
}

#[tokio::test]
async fn subscribe_receives_batch_after_flush() {
    let _guard = setup();
    let mut rx = subscribe_logs_channel();
    push_log("opencode", "info", "a");
    push_log("opencode", "info", "b");
    push_log("rathole", "info", "c");
    flush_now();
    let mut got = Vec::new();
    let deadline = std::time::Instant::now() + Duration::from_secs(2);
    while got.len() < 3 && std::time::Instant::now() < deadline {
        match tokio::time::timeout(Duration::from_millis(200), rx.recv()).await {
            Ok(Some(line)) => got.push(line),
            _ => break,
        }
    }
    assert_eq!(got.len(), 3, "批量收到全部: {got:?}");
    assert_eq!(got[2].message, "c");
}

#[test]
fn recent_n_takes_latest() {
    let _guard = setup();
    for i in 0..10 {
        push_log("system", "info", &format!("m{i}"));
    }
    let logs = recent_logs(3);
    assert_eq!(logs.len(), 3);
    assert!(logs[2].message.contains("m9"), "取最新 N 条");
}

#[test]
fn clear_resets_buffer() {
    let _guard = setup();
    push_log("system", "info", "x");
    clear_logs();
    assert!(recent_logs(100).is_empty());
}

#[tokio::test]
async fn incremental_flush_no_duplicates() {
    let _guard = setup();
    let mut rx = subscribe_logs_channel();
    let replay = drain(&mut rx).await; // 回放历史（空）
    assert!(replay.is_empty());

    push_log("opencode", "info", "n1");
    flush_now();
    let batch1 = drain(&mut rx).await;
    assert_eq!(batch1.len(), 1);

    push_log("opencode", "info", "n2");
    push_log("opencode", "info", "n3");
    flush_now();
    flush_now(); // 二次冲刷不得重发
    let batch2 = drain(&mut rx).await;
    assert_eq!(batch2.len(), 2, "增量只发新增: {batch2:?}");
    assert_eq!(batch2[0].message, "n2");
    assert_eq!(batch2[1].message, "n3");
}

async fn drain(rx: &mut tokio::sync::mpsc::UnboundedReceiver<rust_lib_mini_oc::logbuf::LogLine>) -> Vec<rust_lib_mini_oc::logbuf::LogLine> {
    let mut out = Vec::new();
    let deadline = std::time::Instant::now() + Duration::from_millis(300);
    while std::time::Instant::now() < deadline {
        match rx.try_recv() {
            Ok(l) => out.push(l),
            Err(_) => break,
        }
    }
    out
}
