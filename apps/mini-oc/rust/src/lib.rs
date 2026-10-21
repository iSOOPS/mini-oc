//! mini-oc 客户端 Rust core：经 flutter_rust_bridge 暴露给 Dart。
//!
//! 模块（Task 2-6 填充）：
//! - `api`：frb 入口（暴露给 Dart 的函数与流）
//! - `config`：本地配置持久化
//! - `account`：登录/绑定
//! - `rathole`：rathole 配置生成与二进制定位
//! - `supervisor`：serve/rathole 进程监督状态机
//! - `logbuf`：环形日志缓冲
//! - `term_spawn`：系统终端启动器

mod frb_generated; /* AUTO INJECTED BY flutter_rust_bridge. This line may not be accurate, and you can change it according to your needs. */

pub mod account;
pub mod api;
pub mod config;
pub mod rathole;
