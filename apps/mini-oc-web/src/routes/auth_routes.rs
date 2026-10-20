//! 登录/会话/自助接口（Task 6 填充）。

use crate::routes::SharedState;
use axum::Router;

pub fn router() -> Router<SharedState> {
    Router::new()
}
