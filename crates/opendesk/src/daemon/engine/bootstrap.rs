use std::path::Path;

use anyhow::Context;
use opendesk_core::config::Config;
use opendesk_core::peers::PeerStore;
use tokio::signal::unix::{SignalKind, signal};
use tokio::sync::{mpsc, oneshot};
use tracing::{info, warn};

use super::{Engine, EngineChannels, EnginePaths, EngineSinks};
use crate::daemon::config_watch::spawn_config_watch;
use crate::daemon::identity;
use crate::daemon::ipc::server::spawn_ipc_server;
use crate::daemon::ipc::socket_path;
use crate::daemon::net::discovery::{LocalIdentity, spawn_discovery};
use crate::daemon::net::tcp::spawn_tcp;
use crate::daemon::net::udp::spawn_udp;
use crate::daemon::transfer::remove_expired;

const IPC_QUEUE: usize = 16;

pub async fn run_daemon() -> anyhow::Result<()> {
    let config_path = Config::default_path()?;
    let config =
        Config::load(&config_path).with_context(|| format!("loading {}", config_path.display()))?;
    let peers_path = PeerStore::default_path()?;
    let peer_store = PeerStore::load(&peers_path)
        .with_context(|| format!("loading {}", peers_path.display()))?;
    let peer_id = identity::load_or_create(&config_path.with_file_name("identity.toml"))?;
    remove_expired(
        &config.general.dnd_dir,
        std::time::Duration::from_secs(u64::from(config.general.dnd_keep_days) * 24 * 60 * 60),
    );
    let identity = LocalIdentity {
        peer_id,
        name: config.general.name.clone(),
        port: config.general.port,
        version: env!("CARGO_PKG_VERSION").to_owned(),
    };
    info!(name = %identity.name, %peer_id, port = identity.port, "daemon starting");

    let (wayland_sender, wayland_events) = mpsc::unbounded_channel();
    let wayland = opendesk_wayland::spawn(wayland_sender).context("starting the wayland thread")?;
    let (tcp_sender, tcp_events) = mpsc::unbounded_channel();
    let tcp = spawn_tcp(identity.port, tcp_sender).await?;
    let (udp_sender, udp_events) = mpsc::unbounded_channel();
    let udp = spawn_udp(identity.port, udp_sender).await?;
    let (discovery_sender, discovery_events) = mpsc::unbounded_channel();
    let discovery = spawn_discovery(identity.clone(), discovery_sender)?;
    let ipc_socket = socket_path()?;
    let (ipc_sender, ipc_requests) = mpsc::channel(IPC_QUEUE);
    let ipc_task = spawn_ipc_server(ipc_socket.clone(), ipc_sender).await?;
    let (config_sender, config_events) = mpsc::unbounded_channel();
    let config_watcher = spawn_config_watch(&config_path, config_sender)?;
    let shutdown = spawn_signal_listener()?;

    let sinks = EngineSinks {
        wayland,
        tcp: tcp.commands.clone(),
        udp: udp.commands.clone(),
    };
    let paths = EnginePaths {
        config: config_path,
        peers: peers_path,
    };
    let engine = Engine::new(identity, paths, config, peer_store, sinks);
    let channels = EngineChannels {
        wayland_events,
        tcp_events,
        udp_events,
        discovery_events,
        ipc_requests,
        config_events,
        shutdown,
    };
    let outcome = engine.run(channels).await;

    info!("daemon stopping");
    drop(config_watcher);
    ipc_task.abort();
    remove_socket(&ipc_socket);
    tcp.shutdown();
    udp.shutdown();
    if let Err(error) = discovery.shutdown() {
        warn!(%error, "mdns shutdown failed");
    }
    outcome
}

fn spawn_signal_listener() -> anyhow::Result<oneshot::Receiver<()>> {
    let (sender, receiver) = oneshot::channel();
    let mut terminate = signal(SignalKind::terminate()).context("listening for SIGTERM")?;
    tokio::spawn(async move {
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {}
            _ = terminate.recv() => {}
        }
        let _ = sender.send(());
    });
    Ok(receiver)
}

fn remove_socket(path: &Path) {
    if let Err(error) = std::fs::remove_file(path)
        && error.kind() != std::io::ErrorKind::NotFound
    {
        warn!(%error, path = %path.display(), "ipc socket cleanup failed");
    }
}
