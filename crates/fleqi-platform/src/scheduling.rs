//! 把阻塞的系统 UI 投递到宿主主线程。macOS 选择面板与其它平台对话框共用这一端口。

/// 把闭包投递到 UI 主线程执行。
pub trait MainThreadExecutor: Send + Sync {
    fn run(&self, job: Box<dyn FnOnce() + Send>);
}
