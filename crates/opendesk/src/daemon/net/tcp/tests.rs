use std::net::{Ipv4Addr, SocketAddr};
use std::time::Duration;

use super::{ConnectionId, TcpCommand, TcpEvent, spawn_tcp};
use opendesk_proto::control::ControlMessage;
use tokio::sync::mpsc::{self, UnboundedReceiver};
use tokio::time::timeout;

async fn next(events: &mut UnboundedReceiver<TcpEvent>) -> TcpEvent {
    timeout(Duration::from_secs(5), events.recv())
        .await
        .expect("timed out waiting for a tcp event")
        .expect("tcp event channel closed")
}

fn established(event: TcpEvent) -> Option<(ConnectionId, SocketAddr)> {
    match event {
        TcpEvent::Connected {
            connection,
            address,
        }
        | TcpEvent::Accepted {
            connection,
            address,
        } => Some((connection, address)),
        _ => None,
    }
}

fn closed(event: TcpEvent) -> Option<ConnectionId> {
    match event {
        TcpEvent::Closed { connection, .. } => Some(connection),
        _ => None,
    }
}

fn free_port() -> u16 {
    std::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

#[tokio::test]
async fn two_actors_exchange_pings_and_report_close() {
    let port_a = free_port();
    let port_b = free_port();
    let (events_a, mut receiver_a) = mpsc::unbounded_channel();
    let (events_b, mut receiver_b) = mpsc::unbounded_channel();
    let actor_a = spawn_tcp(port_a, events_a).await.unwrap();
    let actor_b = spawn_tcp(port_b, events_b).await.unwrap();

    let target = SocketAddr::from((Ipv4Addr::LOCALHOST, port_b));
    actor_a
        .commands
        .send(TcpCommand::Connect { address: target })
        .unwrap();

    let (connection_a, connected_to) = established(next(&mut receiver_a).await).unwrap();
    assert_eq!(connected_to, target);
    let (connection_b, _) = established(next(&mut receiver_b).await).unwrap();

    actor_a
        .commands
        .send(TcpCommand::Send {
            connection: connection_a,
            message: ControlMessage::Ping { nonce: 7 },
        })
        .unwrap();
    assert_eq!(
        next(&mut receiver_b).await,
        TcpEvent::Message {
            connection: connection_b,
            message: ControlMessage::Ping { nonce: 7 }
        }
    );

    actor_b
        .commands
        .send(TcpCommand::Send {
            connection: connection_b,
            message: ControlMessage::Pong { nonce: 7 },
        })
        .unwrap();
    assert_eq!(
        next(&mut receiver_a).await,
        TcpEvent::Message {
            connection: connection_a,
            message: ControlMessage::Pong { nonce: 7 }
        }
    );

    actor_a
        .commands
        .send(TcpCommand::Close {
            connection: connection_a,
        })
        .unwrap();
    assert_eq!(closed(next(&mut receiver_a).await), Some(connection_a));
    assert_eq!(closed(next(&mut receiver_b).await), Some(connection_b));

    actor_a.shutdown();
    actor_b.shutdown();
}

#[tokio::test]
async fn connect_to_closed_port_reports_failure_and_unknown_send_is_dropped() {
    let port = free_port();
    let closed_port = free_port();
    let (events, mut receiver) = mpsc::unbounded_channel();
    let actor = spawn_tcp(port, events).await.unwrap();
    let target = SocketAddr::from((Ipv4Addr::LOCALHOST, closed_port));
    actor
        .commands
        .send(TcpCommand::Connect { address: target })
        .unwrap();
    assert!(matches!(
        next(&mut receiver).await,
        TcpEvent::ConnectFailed { address, .. } if address == target
    ));
    actor
        .commands
        .send(TcpCommand::Send {
            connection: ConnectionId(999),
            message: ControlMessage::Ping { nonce: 1 },
        })
        .unwrap();
    assert!(
        timeout(Duration::from_millis(200), receiver.recv())
            .await
            .is_err()
    );
    actor.shutdown();
}
