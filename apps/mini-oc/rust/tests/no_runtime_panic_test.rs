//! I9 复现：frb 非 async 入口在无 tokio 上下文线程上的 panic。
//!
//! 背景：frb 的非 async 函数（wrap_normal，如 service_state / subscribe_state /
//! subscribe_logs）跑在 frb SimpleThreadPool 的**普通线程**上，无 tokio runtime
//! 上下文。旧实现里这些入口会触达裸 `tokio::spawn`：
//! - `service_state` / `subscribe_state` → `global_supervisor()` 首次初始化
//!   （api/mod.rs 内两处裸 spawn：monitor 循环 + 日志 flush 循环）
//! - `subscribe_state` / `subscribe_logs` → 函数体内裸 spawn 桥接循环
//!
//! panic 信息 `must be called from the context of a Tokio 1.x runtime`
//! 被 frb catch_unwind 静默吞掉 → Dart 侧状态流/日志流直接 error →
//! UI 永远显示「已停止」、日志页无日志。
//!
//! 修复：上述裸 spawn 收敛到共享 runtime（crate::runtime::spawn_on_runtime）。

/// service_state 在普通（无 runtime）线程上调用不得 panic。
/// 覆盖 panic 路径：global_supervisor() 首次初始化内的两处裸 tokio::spawn。
#[test]
fn service_state_on_plain_thread_does_not_panic() {
    let h = std::thread::spawn(|| {
        let _ = rust_lib_mini_oc::api::service_state();
    });
    h.join().expect("service_state 在无 runtime 上下文线程上 panic（I9）");
}
