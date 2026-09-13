use std::net::{Ipv4Addr, SocketAddr};
use std::time::Duration;

use super::{UdpCommand, UdpEvent, spawn_udp};
use opendesk_proto::input::{InputDatagram, InputEvent, InputHeader};
use tokio::sync::mpsc;
use tokio::time::timeout;

fn free_udp_port() -> u16 {
    std::net::UdpSocket::bind((Ipv4Addr::LOCALHOST, 0))
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

#[tokio::test]
async fn datagram_sent_from_one_actor_arrives_at_the_other() {
    let port_a = free_udp_port();
    let port_b = free_udp_port();
    let (events_a, _receiver_a) = mpsc::unbounded_channel();
    let (events_b, mut receiver_b) = mpsc::unbounded_channel();
    let actor_a = spawn_udp(port_a, events_a).await.unwrap();
    let actor_b = spawn_udp(port_b, events_b).await.unwrap();

    let datagram = InputDatagram {
        header: InputHeader {
            epoch: None,
            session_id: 4,
            sequence: 12,
        },
        event: InputEvent::Motion { dx: 3.5, dy: -1.0 },
    };
    actor_a
        .commands
        .send(UdpCommand::Send {
            to: SocketAddr::from((Ipv4Addr::LOCALHOST, port_b)),
            datagram,
        })
        .unwrap();

    let event = timeout(Duration::from_secs(5), receiver_b.recv())
        .await
        .unwrap()
        .unwrap();
    match event {
        UdpEvent::Datagram {
            from,
            datagram: received,
        } => {
            assert_eq!(from.port(), port_a);
            assert_eq!(received, datagram);
        }
    }

    actor_a.shutdown();
    actor_b.shutdown();
}

#[tokio::test]
async fn garbage_datagram_is_ignored() {
    let port = free_udp_port();
    let (events, mut receiver) = mpsc::unbounded_channel();
    let actor = spawn_udp(port, events).await.unwrap();
    let sender = tokio::net::UdpSocket::bind((Ipv4Addr::LOCALHOST, 0))
        .await
        .unwrap();
    sender
        .send_to(&[0xff; 64], (Ipv4Addr::LOCALHOST, port))
        .await
        .unwrap();
    assert!(
        timeout(Duration::from_millis(200), receiver.recv())
            .await
            .is_err()
    );
    actor.shutdown();
}
