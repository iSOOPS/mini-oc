//! frb 入口模块：暴露给 Dart 的函数与类型。
//!
//! 只有本命名空间内的 pub 项生成 Dart 绑定（flutter_rust_bridge.yaml 限定 crate::api）。

pub use crate::account::{bind_device, login, logout, AccountInfo, DeviceBrief};
pub use crate::config::{load_config, save_config, ClientConfig, StoredUser};

/// 桥接链路自检（同步）。
#[flutter_rust_bridge::frb(sync)]
pub fn ping() -> String {
    "pong".into()
}

pub mod simple;
