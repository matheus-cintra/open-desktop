use std::collections::HashMap;
use std::net::{IpAddr, Ipv6Addr, SocketAddr};

use anyhow::Context;
use mdns_sd::{ResolvedService, ServiceDaemon, ServiceEvent, ServiceInfo};
use opendesk_proto::SERVICE_TYPE;
use opendesk_proto::control::PeerId;
use tokio::sync::mpsc::UnboundedSender;

use super::service_txt::{ServiceTxt, build_txt, parse_txt};
use crate::daemon::host::hostname;

#[derive(Debug, Clone)]
pub struct LocalIdentity {
    pub peer_id: PeerId,
    pub name: String,
    pub port: u16,
    pub version: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscoveredPeer {
    pub peer_id: PeerId,
    pub name: String,
    pub address: SocketAddr,
    pub version: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DiscoveryEvent {
    Found(DiscoveredPeer),
    Lost(PeerId),
}

pub struct DiscoveryHandle {
    daemon: ServiceDaemon,
    fullname: String,
}

impl DiscoveryHandle {
    pub fn shutdown(self) -> anyhow::Result<()> {
        if let Err(error) = self.daemon.unregister(&self.fullname) {
            tracing::debug!(%error, "mdns unregister failed");
        }
        self.daemon
            .shutdown()
            .context("failed to shut down the mdns daemon")?;
        Ok(())
    }
}

pub fn spawn_discovery(
    identity: LocalIdentity,
    events: UnboundedSender<DiscoveryEvent>,
) -> anyhow::Result<DiscoveryHandle> {
    let daemon = ServiceDaemon::new().context("failed to start the mdns daemon")?;
    let txt = build_txt(&ServiceTxt {
        peer_id: identity.peer_id,
        name: identity.name.clone(),
        version: identity.version.clone(),
        port: identity.port,
    });
    let host = format!("{}.local.", hostname());
    let info = ServiceInfo::new(SERVICE_TYPE, &identity.name, &host, (), identity.port, txt)
        .context("failed to build the mdns service info")?
        .enable_addr_auto();
    let fullname = info.get_fullname().to_owned();
    daemon
        .register(info)
        .context("failed to register the mdns service")?;
    let receiver = daemon
        .browse(SERVICE_TYPE)
        .context("failed to browse for mdns services")?;
    tracing::info!(name = %identity.name, port = identity.port, "mdns registered and browsing");

    let own_peer_id = identity.peer_id;
    std::thread::Builder::new()
        .name("opendesk-mdns".to_owned())
        .spawn(move || bridge_events(&receiver, own_peer_id, &events))
        .context("failed to spawn the mdns bridge thread")?;

    Ok(DiscoveryHandle { daemon, fullname })
}

fn bridge_events(
    receiver: &mdns_sd::Receiver<ServiceEvent>,
    own_peer_id: PeerId,
    events: &UnboundedSender<DiscoveryEvent>,
) {
    let mut known: HashMap<String, PeerId> = HashMap::new();
    while let Ok(event) = receiver.recv() {
        let outgoing = match event {
            ServiceEvent::ServiceResolved(resolved) => {
                match resolved_to_peer(&resolved, own_peer_id) {
                    Some(peer) => {
                        known.insert(resolved.fullname.clone(), peer.peer_id);
                        Some(DiscoveryEvent::Found(peer))
                    }
                    None => None,
                }
            }
            ServiceEvent::ServiceRemoved(_, fullname) => {
                known.remove(&fullname).map(DiscoveryEvent::Lost)
            }
            ServiceEvent::SearchStopped(_) => break,
            _ => None,
        };
        if let Some(outgoing) = outgoing {
            tracing::debug!(?outgoing, "discovery event");
            if events.send(outgoing).is_err() {
                break;
            }
        }
    }
    tracing::debug!("mdns bridge thread finished");
}

fn resolved_to_peer(resolved: &ResolvedService, own_peer_id: PeerId) -> Option<DiscoveredPeer> {
    let txt = match parse_txt(|key| resolved.get_property_val_str(key)) {
        Ok(txt) => txt,
        Err(error) => {
            tracing::debug!(fullname = %resolved.fullname, %error, "ignoring mdns service");
            return None;
        }
    };
    if txt.peer_id == own_peer_id {
        return None;
    }
    let ip = preferred_address(resolved.addresses.iter().map(|scoped| scoped.to_ip_addr()))?;
    Some(DiscoveredPeer {
        peer_id: txt.peer_id,
        name: txt.name,
        address: SocketAddr::new(ip, txt.port),
        version: txt.version,
    })
}

fn is_link_local_v6(address: Ipv6Addr) -> bool {
    address.segments()[0] & 0xffc0 == 0xfe80
}

fn address_rank(address: IpAddr) -> Option<u8> {
    match address {
        IpAddr::V4(value) if value.is_link_local() => None,
        IpAddr::V4(value) if value.is_loopback() => Some(3),
        IpAddr::V4(_) => Some(0),
        IpAddr::V6(value) if is_link_local_v6(value) => None,
        IpAddr::V6(value) if value.is_loopback() => Some(4),
        IpAddr::V6(_) => Some(2),
    }
}

fn preferred_address(addresses: impl Iterator<Item = IpAddr>) -> Option<IpAddr> {
    addresses
        .filter_map(|address| address_rank(address).map(|rank| (rank, address)))
        .min_by_key(|(rank, _)| *rank)
        .map(|(_, address)| address)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{Ipv4Addr, Ipv6Addr};

    #[test]
    fn routable_ipv4_wins_and_link_local_is_skipped() {
        let loopback6 = IpAddr::V6(Ipv6Addr::LOCALHOST);
        let routable4 = IpAddr::V4(Ipv4Addr::new(192, 168, 1, 2));
        let link_local6 = IpAddr::V6(Ipv6Addr::new(0xfe80, 0, 0, 0, 1, 2, 3, 4));
        let link_local4 = IpAddr::V4(Ipv4Addr::new(169, 254, 1, 1));
        let loopback4 = IpAddr::V4(Ipv4Addr::LOCALHOST);
        assert_eq!(
            preferred_address([loopback6, routable4].into_iter()),
            Some(routable4)
        );
        assert_eq!(
            preferred_address([routable4, loopback6].into_iter()),
            Some(routable4)
        );
        assert_eq!(
            preferred_address([link_local6, loopback4].into_iter()),
            Some(loopback4)
        );
        assert_eq!(
            preferred_address([link_local6, link_local4].into_iter()),
            None
        );
        assert_eq!(
            preferred_address([loopback4, loopback6].into_iter()),
            Some(loopback4)
        );
        assert_eq!(preferred_address(std::iter::empty()), None);
    }
}
