//! 管理端接口（Task 10 填充）。

use crate::routes::SharedState;
use axum::Router;

pub fn router() -> Router<SharedState> {
    Router::new()
}
