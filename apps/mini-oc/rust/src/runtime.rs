//! 共享 tokio runtime（I9 修复）。
//!
//! 背景：frb 2.11 非 async 入口（`wrap_normal`：`service_state` /
//! `subscribe_state` / `subscribe_logs` 等）跑在 frb SimpleThreadPool 的普通
//! 线程上，**没有 tokio runtime 上下文**；async 入口虽跑在 frb 的
//! SimpleAsyncRuntime 上，但那是另一套 runtime，与这里互不可见。
//!
//! 因此 crate 内一切「从任意线程发起的后台任务」都必须走本模块的专用 runtime，
//! 不得裸调 `tokio::spawn`（裸调在无上下文线程上会 panic
//! `must be called from the context of a Tokio 1.x runtime`，且该 panic 会被
//! frb catch_unwind 静默吞掉，表现为 Dart 侧状态/日志流直接 error）。

use std::sync::OnceLock;
use tokio::runtime::Runtime;

static RUNTIME: OnceLock<Runtime> = OnceLock::new();

fn runtime() -> &'static Runtime {
    RUNTIME.get_or_init(|| {
        Runtime::new().unwrap_or_else(|e| {
            tracing::error!("共享 tokio runtime 创建失败: {e}");
            panic!("共享 tokio runtime 创建失败: {e}");
        })
    })
}

/// 在共享 runtime 上派生后台任务（可在任意线程调用，含无 tokio 上下文的线程）。
pub fn spawn_on_runtime<F>(task: F) -> tokio::task::JoinHandle<F::Output>
where
    F: std::future::Future + Send + 'static,
    F::Output: Send + 'static,
{
    runtime().spawn(task)
}
