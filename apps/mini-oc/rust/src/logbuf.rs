//! 环形日志缓冲：子进程输出、系统事件统一汇入；100ms 节流批量增量推流。
//!
//! 数据流：`push_log` → VecDeque(2000) + 单调序列号 → 脏标记 →
//! flush（只发上次之后的新增）→ 订阅通道。
//! （api 层把通道桥接为 frb StreamSink；节流循环由 api 初始化时启动。）

use serde::Serialize;
use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Mutex, OnceLock};
use tokio::sync::mpsc;

/// 每条日志的结构。
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct LogLine {
    pub ts_ms: i64,
    pub source: String,
    pub level: String,
    pub message: String,
}

/// 环形容量上限。
const CAPACITY: usize = 2000;
/// 订阅时回放的历史条数。
const REPLAY_ON_SUBSCRIBE: usize = 200;
/// 节流窗口。
pub const FLUSH_INTERVAL_MS: u64 = 100;

struct LogBuffer {
    inner: Mutex<Inner>,
    /// 单调写入总数（全局序列号上界）
    total: AtomicUsize,
    /// 已推送到订阅者的序列号
    last_sent: AtomicUsize,
    dirty: AtomicBool,
    tx: Mutex<mpsc::UnboundedSender<LogLine>>,
}

struct Inner {
    /// ring[0] 的全局序列号
    base: usize,
    ring: VecDeque<LogLine>,
}

fn buffer() -> &'static LogBuffer {
    static BUF: OnceLock<LogBuffer> = OnceLock::new();
    BUF.get_or_init(|| {
        let (tx, _rx) = mpsc::unbounded_channel();
        LogBuffer {
            inner: Mutex::new(Inner {
                base: 0,
                ring: VecDeque::with_capacity(CAPACITY),
            }),
            total: AtomicUsize::new(0),
            last_sent: AtomicUsize::new(0),
            dirty: AtomicBool::new(false),
            tx: Mutex::new(tx),
        }
    })
}

/// 写入一条日志（多来源并发安全）。
pub fn push_log(source: &str, level: &str, message: &str) {
    let line = LogLine {
        ts_ms: chrono::Utc::now().timestamp_millis(),
        source: source.to_string(),
        level: level.to_string(),
        message: message.to_string(),
    };
    let mut inner = buffer().inner.lock().unwrap();
    if inner.ring.len() == CAPACITY {
        inner.ring.pop_front();
        inner.base += 1;
    }
    inner.ring.push_back(line);
    buffer().total.fetch_add(1, Ordering::AcqRel);
    buffer().dirty.store(true, Ordering::Release);
}

/// 最近 N 条（最新在后）。
pub fn recent_logs(n: usize) -> Vec<LogLine> {
    let inner = buffer().inner.lock().unwrap();
    let start = inner.ring.len().saturating_sub(n);
    inner.ring.iter().skip(start).cloned().collect()
}

/// 清空（序列号继续前进，避免旧推送混淆）。
pub fn clear_logs() {
    let mut inner = buffer().inner.lock().unwrap();
    inner.base = buffer().total.load(Ordering::Acquire);
    inner.ring.clear();
    buffer().last_sent.store(inner.base, Ordering::Release);
}

/// 新订阅者通道（接管旧通道；回放最近 200 条历史作为冷启动上下文）。
pub fn subscribe_logs_channel() -> mpsc::UnboundedReceiver<LogLine> {
    let (tx, rx) = mpsc::unbounded_channel();
    *buffer().tx.lock().unwrap() = tx;
    let total = buffer().total.load(Ordering::Acquire);
    let start = total.saturating_sub(REPLAY_ON_SUBSCRIBE);
    send_range(start, total);
    buffer().last_sent.store(total, Ordering::Release);
    rx
}

/// 发送全局序列号 [from, to) 范围内仍在 ring 中的行。
fn send_range(from: usize, to: usize) {
    let inner = buffer().inner.lock().unwrap();
    let tx = buffer().tx.lock().unwrap().clone();
    let lo = from.max(inner.base);
    for seq in lo..to {
        if let Some(line) = inner.ring.get(seq - inner.base) {
            let _ = tx.send(line.clone());
        }
    }
}

/// 立即冲刷：只推送上次之后的新增（节流循环每 100ms 调用；测试可直接调用）。
pub fn flush_now() {
    if !buffer().dirty.swap(false, Ordering::AcqRel) {
        return;
    }
    let last = buffer().last_sent.load(Ordering::Acquire);
    let total = buffer().total.load(Ordering::Acquire);
    if total > last {
        send_range(last, total);
        buffer().last_sent.store(total, Ordering::Release);
    }
}

/// 节流冲刷循环（api 初始化时 tokio::spawn）。
pub async fn flush_loop() {
    loop {
        tokio::time::sleep(std::time::Duration::from_millis(FLUSH_INTERVAL_MS)).await;
        flush_now();
    }
}
