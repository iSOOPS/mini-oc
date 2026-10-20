//! 设备信息/绑定/列表/跳转接口（Task 7-9 填充）。

use crate::routes::SharedState;
use axum::Router;

pub fn router() -> Router<SharedState> {
    Router::new()
}
