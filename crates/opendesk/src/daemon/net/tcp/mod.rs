mod connection;

use std::collections::HashMap;
use std::net::{Ipv4Addr, SocketAddr};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use anyhow::Context;
use opendesk_proto::control::ControlMessage;
use tokio::net::{TcpListener, TcpSocket, TcpStream};
use tokio::sync::mpsc::{self, UnboundedReceiver, UnboundedSender};
use tokio::task::JoinHandle;

use connection::{ConnectionControl, run_connection};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ConnectionId(pub u64);

#[derive(Debug, Clone, PartialEq)]
pub enum TcpEvent {
    Accepted {
        connection: ConnectionId,
        address: SocketAddr,
    },
    Connected {
        connection: ConnectionId,
        address: SocketAddr,
    },
    ConnectFailed {
        address: SocketAddr,
        error: String,
    },
    Message {
        connection: ConnectionId,
        message: ControlMessage,
    },
    Closed {
        connection: ConnectionId,
        reason: String,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub enum TcpCommand {
    Connect {
        address: SocketAddr,
    },
    Send {
        connection: ConnectionId,
        message: ControlMessage,
    },
    Close {
        connection: ConnectionId,
    },
}

pub struct TcpHandle {
    pub commands: UnboundedSender<TcpCommand>,
    task: JoinHandle<()>,
}

impl TcpHandle {
    pub fn shutdown(self) {
        self.task.abort();
    }
}

type ConnectionTable = Arc<Mutex<HashMap<ConnectionId, ConnectionControl>>>;

#[derive(Clone)]
struct Shared {
    events: UnboundedSender<TcpEvent>,
    connections: ConnectionTable,
    next_id: Arc<AtomicU64>,
}

impl Shared {
    fn allocate_id(&self) -> ConnectionId {
        ConnectionId(self.next_id.fetch_add(1, Ordering::Relaxed))
    }

    fn register(&self, id: ConnectionId, control: ConnectionControl) {
        if let Ok(mut table) = self.connections.lock() {
            table.insert(id, control);
        }
    }

    fn remove(&self, id: ConnectionId) -> Option<ConnectionControl> {
        self.connections.lock().ok()?.remove(&id)
    }

    fn with_control<T>(
        &self,
        id: ConnectionId,
        apply: impl FnOnce(&ConnectionControl) -> T,
    ) -> Option<T> {
        self.connections.lock().ok()?.get(&id).map(apply)
    }
}

pub async fn spawn_tcp(port: u16, events: UnboundedSender<TcpEvent>) -> anyhow::Result<TcpHandle> {
    let listener = bind_listener(port)?;
    let (commands, command_receiver) = mpsc::unbounded_channel();
    let shared = Shared {
        events,
        connections: Arc::default(),
        next_id: Arc::new(AtomicU64::new(1)),
    };
    tracing::info!(port, "tcp listener bound");
    let task = tokio::spawn(actor_loop(listener, command_receiver, shared));
    Ok(TcpHandle { commands, task })
}

fn bind_listener(port: u16) -> anyhow::Result<TcpListener> {
    let socket = TcpSocket::new_v4().context("failed to create the tcp socket")?;
    socket
        .set_reuseaddr(true)
        .context("failed to set SO_REUSEADDR")?;
    socket
        .set_nodelay(true)
        .context("failed to set TCP_NODELAY")?;
    let address = SocketAddr::from((Ipv4Addr::UNSPECIFIED, port));
    socket
        .bind(address)
        .with_context(|| format!("failed to bind tcp port {port}"))?;
    socket
        .listen(64)
        .context("failed to listen on the tcp port")
}

async fn actor_loop(
    listener: TcpListener,
    mut commands: UnboundedReceiver<TcpCommand>,
    shared: Shared,
) {
    loop {
        tokio::select! {
            accepted = listener.accept() => match accepted {
                Ok((stream, address)) => start_connection(&shared, stream, address, true),
                Err(error) => {
                    tracing::warn!(%error, "tcp accept failed");
                    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                }
            },
            command = commands.recv() => match command {
                Some(command) => handle_command(&shared, command),
                None => break,
            },
        }
    }
    tracing::debug!("tcp actor finished");
}

fn handle_command(shared: &Shared, command: TcpCommand) {
    match command {
        TcpCommand::Connect { address } => {
            tokio::spawn(dial(shared.clone(), address));
        }
        TcpCommand::Send {
            connection,
            message,
        } => {
            let delivered = shared.with_control(connection, |control| control.send(&message));
            match delivered {
                Some(Ok(())) => {}
                Some(Err(error)) => {
                    tracing::warn!(?connection, %error, "tcp send failed, closing connection");
                    shared.with_control(connection, ConnectionControl::request_close);
                }
                None => tracing::warn!(?connection, "tcp send to unknown connection dropped"),
            }
        }
        TcpCommand::Close { connection } => {
            if shared
                .with_control(connection, ConnectionControl::request_close)
                .is_none()
            {
                tracing::debug!(?connection, "tcp close of unknown connection ignored");
            }
        }
    }
}

async fn dial(shared: Shared, address: SocketAddr) {
    match TcpStream::connect(address).await {
        Ok(stream) => start_connection(&shared, stream, address, false),
        Err(error) => {
            tracing::debug!(%address, %error, "tcp connect failed");
            let _ = shared.events.send(TcpEvent::ConnectFailed {
                address,
                error: error.to_string(),
            });
        }
    }
}

fn start_connection(shared: &Shared, stream: TcpStream, address: SocketAddr, inbound: bool) {
    if let Err(error) = stream.set_nodelay(true) {
        tracing::warn!(%address, %error, "failed to set TCP_NODELAY");
    }
    let id = shared.allocate_id();
    let (control, outbound) = ConnectionControl::new();
    shared.register(id, control);
    let event = if inbound {
        TcpEvent::Accepted {
            connection: id,
            address,
        }
    } else {
        TcpEvent::Connected {
            connection: id,
            address,
        }
    };
    tracing::info!(?id, %address, inbound, "tcp connection established");
    let _ = shared.events.send(event);
    let shared = shared.clone();
    tokio::spawn(async move {
        let reason = run_connection(stream, outbound, &shared.events, id).await;
        shared.remove(id);
        tracing::info!(?id, %reason, "tcp connection closed");
        let _ = shared.events.send(TcpEvent::Closed {
            connection: id,
            reason,
        });
    });
}

#[cfg(test)]
mod tests;
