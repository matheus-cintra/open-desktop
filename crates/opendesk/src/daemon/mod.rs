pub mod config_watch;
pub mod engine;
pub mod host;
pub mod identity;
pub mod ipc;
pub mod net;
#[cfg(target_os = "linux")]
pub mod notify;
#[cfg(target_os = "macos")]
#[path = "notify_macos.rs"]
pub mod notify;
pub mod transfer;

pub async fn run() -> anyhow::Result<()> {
    engine::run_daemon().await
}
