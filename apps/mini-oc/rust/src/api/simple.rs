#[flutter_rust_bridge::frb(sync)] // Synchronous mode for simplicity of the demo
pub fn greet(name: String) -> String {
    format!("Hello, {name}!")
}

#[flutter_rust_bridge::frb(init)]
pub fn init_app() {
    // Default utilities - feel free to customize
    flutter_rust_bridge::setup_default_user_utils();
    // 清理异常关闭残留的孤儿 rathole 进程（否则新客户端启动 rathole 因 service 被占而失败）
    crate::portkill::cleanup_orphan_rathole();
}
