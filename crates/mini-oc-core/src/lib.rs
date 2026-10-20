//! mini-oc-core：mini-oc-web 与 mini-oc 客户端共享的核心库。
//!
//! 模块：
//! - `device`：用户/设备数据模型与 V1→V2 迁移
//! - `tunnel`：隧道地址拼装纯函数
//! - `ocapi`：opencode v2 HTTP API 客户端

pub mod device;
pub mod ocapi;
pub mod tunnel;
