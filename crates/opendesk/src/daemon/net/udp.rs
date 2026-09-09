use std::net::{Ipv4Addr, SocketAddr};

use anyhow::Context;
use opendesk_proto::codec::{MAX_DATAGRAM_BYTES, decode_datagram, encode_datagram};
use opendesk_proto::input::InputDatagram;
use tokio::net::UdpSocket;
use tokio::sync::mpsc::{self, UnboundedReceiver, UnboundedSender};
use tokio::task::JoinHandle;

#[derive(Debug, Clone, PartialEq)]
pub enum UdpEvent {
    Datagram {
        from: SocketAddr,
        datagram: InputDatagram,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub enum UdpCommand {
    Send {
        to: SocketAddr,
        datagram: InputDatagram,
    },
}

pub struct UdpHandle {
    pub commands: UnboundedSender<UdpCommand>,
    task: JoinHandle<()>,
}

impl UdpHandle {
    pub fn shutdown(self) {
        self.task.abort();
    }
}

pub async fn spawn_udp(port: u16, events: UnboundedSender<UdpEvent>) -> anyhow::Result<UdpHandle> {
    let socket = UdpSocket::bind(SocketAddr::from((Ipv4Addr::UNSPECIFIED, port)))
        .await
        .with_context(|| format!("failed to bind udp port {port}"))?;
    tracing::info!(port, "udp socket bound");
    let (commands, command_receiver) = mpsc::unbounded_channel();
    let task = tokio::spawn(actor_loop(socket, command_receiver, events));
    Ok(UdpHandle { commands, task })
}

async fn actor_loop(
    socket: UdpSocket,
    mut commands: UnboundedReceiver<UdpCommand>,
    events: UnboundedSender<UdpEvent>,
) {
    let mut buffer = vec![0u8; MAX_DATAGRAM_BYTES * 4];
    let mut undecodable: u64 = 0;
    loop {
        tokio::select! {
            received = socket.recv_from(&mut buffer) => match received {
                Ok((bytes_read, from)) => {
                    let bytes = buffer.get(..bytes_read).unwrap_or_default();
                    match decode_datagram(bytes) {
                        Ok(datagram) => {
                            if events.send(UdpEvent::Datagram { from, datagram }).is_err() {
                                break;
                            }
                        }
                        Err(error) => {
                            undecodable += 1;
                            tracing::debug!(%from, %error, undecodable, "udp datagram dropped");
                        }
                    }
                }
                Err(error) => tracing::debug!(%error, "udp receive failed"),
            },
            command = commands.recv() => match command {
                Some(UdpCommand::Send { to, datagram }) => send_datagram(&socket, to, &datagram).await,
                None => break,
            },
        }
    }
    tracing::debug!("udp actor finished");
}

async fn send_datagram(socket: &UdpSocket, to: SocketAddr, datagram: &InputDatagram) {
    match encode_datagram(datagram) {
        Ok(bytes) => {
            if let Err(error) = socket.send_to(&bytes, to).await {
                tracing::debug!(%to, %error, "udp send failed");
            }
        }
        Err(error) => tracing::warn!(%error, "udp datagram could not be encoded"),
    }
}

#[cfg(test)]
mod tests;
