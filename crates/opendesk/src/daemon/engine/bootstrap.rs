use std::path::Path;
#[cfg(target_os = "linux")]
use std::path::PathBuf;
#[cfg(target_os = "linux")]
use std::time::Duration;
use std::time::Instant;

use crate::platform::{BarStyle, HotkeySpec};
use anyhow::Context;
use opendesk_core::color::Rgba;
use opendesk_core::config::Config;
use opendesk_core::hotkey::Hotkey;
use opendesk_core::peers::PeerStore;
#[cfg(target_os = "linux")]
use tokio::io::{AsyncBufReadExt, BufReader};
#[cfg(target_os = "linux")]
use tokio::net::UnixStream;
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

pub enum CompositorEvent {
    LeftReleased(Instant),
    EmergencyRelease,
}

pub async fn run_daemon() -> anyhow::Result<()> {
    let config_path = Config::default_path()?;
    let config =
        Config::load(&config_path).with_context(|| format!("loading {}", config_path.display()))?;
    opendesk_core::desktop_map::load(&config_path.with_file_name("map.json"))
        .map_err(anyhow::Error::msg)?;
    opendesk_core::desktop_map::load_control_clock(
        &config_path.with_file_name("control-clock.json"),
    )
    .map_err(anyhow::Error::msg)?;
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
    let wayland = crate::platform::spawn(wayland_sender).context("starting the wayland thread")?;
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
    let compositor_events = spawn_compositor_listener().await?;
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
        compositor_events,
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

#[cfg(target_os = "linux")]
async fn spawn_compositor_listener() -> anyhow::Result<mpsc::UnboundedReceiver<CompositorEvent>> {
    let runtime = std::env::var_os("XDG_RUNTIME_DIR")
        .filter(|value| !value.is_empty())
        .context("XDG_RUNTIME_DIR is needed for Hyprland's event socket")?;
    let signature = std::env::var_os("HYPRLAND_INSTANCE_SIGNATURE")
        .filter(|value| !value.is_empty())
        .context("HYPRLAND_INSTANCE_SIGNATURE is needed for Hyprland's event socket")?;
    let path = PathBuf::from(runtime)
        .join("hypr")
        .join(signature)
        .join(".socket2.sock");
    let stream = UnixStream::connect(&path)
        .await
        .with_context(|| format!("connecting to Hyprland events at {}", path.display()))?;
    let (sender, receiver) = mpsc::unbounded_channel();
    tokio::spawn(async move {
        let mut stream = Some(stream);
        loop {
            let connected = match stream.take() {
                Some(connected) => connected,
                None => match UnixStream::connect(&path).await {
                    Ok(connected) => connected,
                    Err(error) => {
                        warn!(%error, "Hyprland event socket reconnect failed");
                        tokio::time::sleep(Duration::from_secs(1)).await;
                        continue;
                    }
                },
            };
            let mut lines = BufReader::new(connected).lines();
            loop {
                match lines.next_line().await {
                    Ok(Some(line)) => {
                        let event = match line.as_str() {
                            "custom>>opendesk-left-release" => {
                                tracing::debug!("Hyprland left-button release event received");
                                Some(CompositorEvent::LeftReleased(Instant::now()))
                            }
                            "custom>>opendesk-emergency-release" => {
                                tracing::debug!("Hyprland emergency release event received");
                                Some(CompositorEvent::EmergencyRelease)
                            }
                            _ => None,
                        };
                        if let Some(event) = event
                            && sender.send(event).is_err()
                        {
                            return;
                        }
                    }
                    Ok(None) => break,
                    Err(error) => {
                        warn!(%error, "Hyprland event socket read failed");
                        break;
                    }
                }
            }
            tokio::time::sleep(Duration::from_millis(200)).await;
        }
    });
    Ok(receiver)
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

pub(super) fn hotkey_spec(hotkey: &Hotkey) -> HotkeySpec {
    HotkeySpec {
        ctrl: hotkey.ctrl,
        alt: hotkey.alt,
        shift: hotkey.shift,
        logo: hotkey.logo,
        key: hotkey.key.clone(),
    }
}

pub(super) fn bar_style(color: &Rgba) -> BarStyle {
    BarStyle {
        red: color.red,
        green: color.green,
        blue: color.blue,
        alpha: color.alpha,
    }
}

#[cfg(target_os = "macos")]
async fn spawn_compositor_listener() -> anyhow::Result<mpsc::UnboundedReceiver<CompositorEvent>> {
    let (_sender, receiver) = mpsc::unbounded_channel();
    Ok(receiver)
}
