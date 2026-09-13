use super::Engine;
use crate::{
    daemon::ipc::{IpcResponse, MapPeer, MapReport},
    platform::PlatformCommand,
};
use opendesk_core::{desktop_map, session::SessionEvent};
use opendesk_proto::{
    control::{ControlMessage, PeerId, Side},
    map::{ControlEpoch, DesktopMap, MapControl, Revision, Screen},
};
use std::collections::HashSet;
use std::time::{Duration, Instant};

#[derive(Clone)]
pub(super) struct Pending {
    pub epoch: ControlEpoch,
    pub next: PeerId,
    pub old: Option<PeerId>,
    pub side: Side,
    pub fraction: f32,
    pub since: Instant,
    pub stage: u8,
}
pub(super) struct MapState {
    pub current: Option<DesktopMap>,
    pub error: Option<String>,
    pub synced: HashSet<PeerId>,
    pub clock: u64,
    pub claim: Revision,
    pub epoch: Option<ControlEpoch>,
    pub highest: Option<ControlEpoch>,
    pub released: Option<ControlEpoch>,
    pub last_status: Instant,
    pub pending: Option<Pending>,
    pub drag_released: bool,
    pub prepared_drag: Option<opendesk_proto::transfer::DragInfo>,
    pub prepared: Option<(PeerId, ControlEpoch, Side, f32, Instant)>,
    pub last_renew: Instant,
    pub last_sent: Instant,
    pub edge_since: Option<(Side, f32, f64)>,
}
impl MapState {
    pub fn new(path: &std::path::Path, local: PeerId) -> Self {
        let loaded = desktop_map::load(path);
        Self {
            current: loaded.clone().ok().flatten(),
            error: loaded.err(),
            synced: HashSet::new(),
            clock: desktop_map::load_control_clock(&path.with_file_name("control-clock.json"))
                .unwrap_or(0),
            claim: Revision {
                counter: 0,
                author: local,
            },
            epoch: None,
            highest: None,
            released: None,
            last_status: Instant::now() - Duration::from_secs(1),
            pending: None,
            prepared: None,
            prepared_drag: None,
            drag_released: false,
            last_renew: Instant::now(),
            last_sent: Instant::now(),
            edge_since: None,
        }
    }
}

impl Engine {
    fn can_receive_map(&self, peer: PeerId) -> bool {
        self.enabled
            && self.monitor.known(Instant::now())
            && self.monitor.lock == super::monitor::LockState::Unlocked
            && self.map.synced.contains(&peer)
    }

    pub(super) fn observe_control_clock(&mut self, counter: u64) -> bool {
        if counter <= self.map.clock {
            return true;
        }
        if let Err(error) = desktop_map::save_control_clock(
            &self.paths.config.with_file_name("control-clock.json"),
            counter,
        ) {
            self.stop_map_control();
            self.enabled = false;
            self.map.error = Some(format!(
                "Não foi possível persistir o contador de controle: {error}"
            ));
            return false;
        }
        self.map.clock = counter;
        true
    }
    fn next_claim(&mut self) -> Option<Revision> {
        let counter = self.map.clock.max(self.map.claim.counter).checked_add(1)?;
        self.observe_control_clock(counter).then_some(Revision {
            counter,
            author: self.identity.peer_id,
        })
    }

    pub(super) fn map_report(&self) -> MapReport {
        let peers = std::iter::once(MapPeer {
            input_status: if self.enabled {
                self.input_status()
            } else {
                "paused"
            }
            .into(),
            peer: self.identity.peer_id,
            name: self.identity.name.clone(),
            connected: true,
            trusted: true,
            ready: self.enabled
                && self.monitor.known(Instant::now())
                && self.monitor.lock == super::monitor::LockState::Unlocked,
            local: true,
        })
        .chain(self.peer_store.records().iter().map(|p| {
            MapPeer {
                input_status: self
                    .links
                    .link(p.id)
                    .map_or_else(|| "unavailable".into(), |l| l.input_status.clone()),
                peer: p.id,
                name: p.name.clone(),
                connected: self.links.link(p.id).is_some(),
                trusted: true,
                ready: self.map.synced.contains(&p.id)
                    && self.links.link(p.id).is_some_and(|l| {
                        l.map_ready && l.last_ready.elapsed() < Duration::from_secs(1)
                    }),
                local: false,
            }
        }))
        .collect();
        MapReport {
            map: self
                .map
                .current
                .clone()
                .unwrap_or_else(|| self.initial_map()),
            applied: self.map.current.is_some(),
            peers,
            error: self.map.error.clone(),
            owner: self.map.epoch.map(|e| e.claim.author),
            target: if self.session.is_controlled() {
                Some(self.identity.peer_id)
            } else {
                self.active_peer
            },
        }
    }
    fn initial_map(&self) -> DesktopMap {
        let local = self.identity.peer_id;
        let mut screens = Vec::new();
        let mut records: Vec<_> = self.peer_store.records().iter().collect();
        records.sort_by_key(|r| r.id);
        let left: Vec<_> = records
            .iter()
            .filter(|r| {
                self.config
                    .side_for_peer(&r.name)
                    .is_some_and(|s| s.covers(Side::Left))
            })
            .collect();
        let ordered: Vec<_> = left
            .iter()
            .map(|r| (r.id, r.name.clone()))
            .chain(std::iter::once((local, self.identity.name.clone())))
            .chain(
                records
                    .iter()
                    .filter(|r| !left.iter().any(|l| l.id == r.id))
                    .map(|r| (r.id, r.name.clone())),
            )
            .collect();
        let mut x = 0;
        for (peer, name) in ordered {
            let output = if peer == local {
                self.outputs.first()
            } else {
                self.links.link(peer).and_then(|l| l.layout.first())
            };
            let (width, height) =
                output.map_or((1920, 1080), |o| (o.width.max(1), o.height.max(1)));
            screens.push(Screen {
                peer,
                name,
                x,
                y: 0,
                width,
                height,
            });
            x += width;
        }
        DesktopMap {
            group: screens.iter().map(|s| s.peer).min().unwrap_or(local),
            revision: Revision {
                counter: 0,
                author: local,
            },
            screens,
        }
    }
    pub(super) fn apply_map(&mut self, base: Option<Revision>, mut map: DesktopMap) -> IpcResponse {
        if self.map.current.as_ref().map(|m| m.revision) != base {
            self.preserve_map(&map);
            return IpcResponse::MapConflict(self.map_report());
        }
        if !map.screens.iter().any(|s| s.peer == self.identity.peer_id) {
            return IpcResponse::Error {
                message: "Inclua este computador no mapa".into(),
            };
        }
        if let Some(current) = &self.map.current {
            map.group = current.group;
        }
        map.revision = match desktop_map::next_revision(base, self.identity.peer_id) {
            Ok(r) => r,
            Err(message) => return IpcResponse::Error { message },
        };
        if let Err(message) = self.install_map(map.clone()) {
            return IpcResponse::Error { message };
        }
        self.broadcast(ControlMessage::Map(MapControl::Sync(Some(map))));
        IpcResponse::Map(self.map_report())
    }
    fn preserve_map(&mut self, map: &DesktopMap) {
        let path = self.paths.config.with_file_name(format!(
            "map-conflict-{}-{}.json",
            map.revision.counter, map.revision.author
        ));
        if let Err(error) = desktop_map::save(&path, map) {
            self.map.error = Some(error);
        }
    }
    fn install_map(&mut self, map: DesktopMap) -> Result<(), String> {
        desktop_map::validate(&map)?;
        self.stop_map_control();
        desktop_map::save(&self.paths.config.with_file_name("map.json"), &map)?;
        self.map.current = Some(map);
        self.map.synced.clear();
        self.reconfigure_strips();
        Ok(())
    }
    pub(super) fn map_destination(
        &self,
        from: PeerId,
        side: Side,
        fraction: f32,
    ) -> Option<desktop_map::Crossing> {
        desktop_map::resolve(self.map.current.as_ref()?, from, side, fraction, |p| {
            p == self.identity.peer_id
                || (self.map.synced.contains(&p)
                    && self.links.link(p).is_some_and(|l| {
                        l.map_ready && l.last_ready.elapsed() < Duration::from_secs(1)
                    })
                    && self.peer_store.find_by_id(p).is_some())
        })
    }
    pub(super) fn stop_map_control(&mut self) {
        if let Some(pending) = self.map.pending.take() {
            self.send_to(
                pending.next,
                ControlMessage::Map(MapControl::Revoke {
                    epoch: pending.epoch,
                }),
            );
        }
        if let Some(epoch) = self.map.epoch {
            self.broadcast(ControlMessage::Map(MapControl::Revoke { epoch }));
        }
        self.dispatch(SessionEvent::HotkeyPressed);
        self.map.epoch = None;
        self.map.prepared = None;
        self.map.prepared_drag = None;
        self.pending_transfer = None;
        self.map.drag_released = false;
        self.map.edge_since = None;
    }
    pub(super) fn claim_physical(&mut self) {
        if self.map.current.is_none()
            || self.monitor.lock != super::monitor::LockState::Unlocked
            || !self.monitor.known(Instant::now())
        {
            return;
        }
        if self.map.claim.author == self.identity.peer_id {
            return;
        }
        self.stop_map_control();
        if let Some(revision) = self.next_claim() {
            self.map.claim = revision;
            self.broadcast(ControlMessage::Map(MapControl::Claim { revision }));
        }
    }
    pub(super) fn begin_map_transfer(
        &mut self,
        next: PeerId,
        side: Side,
        fraction: f32,
        old: Option<PeerId>,
    ) {
        let returning_drag = next == self.identity.peer_id
            && self
                .authorized_return_drop
                .as_ref()
                .is_some_and(|auth| Some(auth.peer) == old);
        if self.map.pending.is_some()
            || !returning_drag
                && (!self.forwarded.can_transfer() || old.is_some() && self.has_file_drag())
        {
            return;
        }
        let Some(revision) = self.map.current.as_ref().map(|m| m.revision) else {
            return;
        };
        if old.is_none() {
            if let Some(claim) = self.next_claim() {
                self.map.claim = claim;
                self.broadcast(ControlMessage::Map(MapControl::Claim { revision: claim }));
            } else {
                self.stop_map_control();
                return;
            }
        }
        let epoch = match self.map.epoch {
            Some(e) => ControlEpoch {
                generation: match e.generation.checked_add(1) {
                    Some(g) => g,
                    None => {
                        self.stop_map_control();
                        return;
                    }
                },
                ..e
            },
            None => ControlEpoch {
                claim: self.map.claim,
                session: self.map.claim.counter,
                generation: 1,
            },
        };
        let drag = self.take_pending_drag_info();
        self.map.pending = Some(Pending {
            epoch,
            next,
            old,
            side,
            fraction,
            since: Instant::now(),
            stage: 0,
        });
        if next == self.identity.peer_id {
            self.map_prepared(next, epoch, true);
        } else {
            self.send_to(
                next,
                ControlMessage::Map(MapControl::Prepare {
                    epoch,
                    revision,
                    drag,
                    side,
                    fraction,
                }),
            );
        }
    }
    fn map_prepared(&mut self, peer: PeerId, epoch: ControlEpoch, accepted: bool) {
        let Some(p) = self
            .map
            .pending
            .clone()
            .filter(|p| p.next == peer && p.epoch == epoch && p.stage == 0)
        else {
            return;
        };
        if !accepted {
            self.stop_map_control();
            return;
        }
        if let Some(pending) = self.map.pending.as_mut() {
            pending.stage = 1;
        }
        if let Some(old) = p.old {
            if let Some(previous) = self.map.epoch {
                self.send_to(
                    old,
                    ControlMessage::Map(MapControl::Release { epoch: previous }),
                );
            }
        } else {
            self.commit_map_transfer();
        }
    }
    fn commit_map_transfer(&mut self) {
        let Some(p) = self.map.pending.clone() else {
            return;
        };
        if let Some(pending) = self.map.pending.as_mut() {
            pending.stage = 2;
        }
        if p.next == self.identity.peer_id {
            let hint = self.edges.hint(p.side.opposite(), p.fraction);
            if let Some(old) = p.old {
                self.mark_return_drag_released(old);
                self.dispatch(SessionEvent::PeerReleased {
                    peer: old,
                    fraction: None,
                });
            }
            self.wayland(PlatformCommand::StopGrab { hint });
            if let Some((x, y)) = self.edges.entry_point(p.side.opposite(), p.fraction) {
                self.wayland(PlatformCommand::InjectAbsoluteMotion { x, y });
            }
            self.map.epoch = None;
            self.map.pending = None;
        } else {
            self.send_to(
                p.next,
                ControlMessage::Map(MapControl::Commit { epoch: p.epoch }),
            );
        }
    }
    pub(super) fn on_map_message(&mut self, peer: PeerId, message: MapControl) {
        match message {
            MapControl::Clock { counter } => {
                self.observe_control_clock(counter);
            }
            MapControl::Sync(incoming) => {
                if let Some(map) = incoming {
                    if let Some(current) = &self.map.current {
                        if current.group != map.group
                            || current.revision == map.revision && current != &map
                        {
                            self.preserve_map(&map);
                            self.map.error=Some("Conflito de grupo ou revisão; mapa recebido preservado para revisão".into());
                            return;
                        }
                        if current.revision.counter == map.revision.counter
                            && map.revision < current.revision
                        {
                            self.preserve_map(&map);
                        }
                    }
                    if desktop_map::validate(&map).is_err()
                        || !map.screens.iter().any(|s| s.peer == self.identity.peer_id)
                    {
                        return;
                    }
                    let replace = self
                        .map
                        .current
                        .as_ref()
                        .is_none_or(|current| map.revision > current.revision);
                    if replace {
                        if let Some(previous) = self.map.current.clone() {
                            self.preserve_map(&previous);
                            self.map.error = Some(
                                "Mapa atualizado em outro computador; versão anterior preservada"
                                    .into(),
                            );
                        }
                        if let Err(error) = self.install_map(map.clone()) {
                            self.map.error = Some(error);
                            return;
                        }
                        self.broadcast(ControlMessage::Map(MapControl::Sync(Some(map))));
                    }
                    if let Some(current) = &self.map.current {
                        self.send_to(
                            peer,
                            ControlMessage::Map(MapControl::Synced {
                                revision: current.revision,
                            }),
                        );
                    }
                } else if let Some(map) = &self.map.current {
                    self.send_to(
                        peer,
                        ControlMessage::Map(MapControl::Sync(Some(map.clone()))),
                    );
                }
            }
            MapControl::Synced { revision } => {
                if self
                    .map
                    .current
                    .as_ref()
                    .is_some_and(|m| m.revision == revision)
                {
                    self.map.synced.insert(peer);
                } else {
                    self.send_to(
                        peer,
                        ControlMessage::Map(MapControl::Sync(self.map.current.clone())),
                    );
                }
            }
            MapControl::Availability {
                ready,
                input_status,
            } => {
                if let Some(link) = self.links.link_mut(peer) {
                    link.map_ready = ready;
                    link.input_status = input_status.chars().take(64).collect();
                    link.last_ready = Instant::now();
                }
            }
            MapControl::Identify => self.identify_local(),
            MapControl::Claim { revision }
                if revision.author == peer && revision > self.map.claim =>
            {
                if revision.counter < self.map.clock {
                    self.send_to(
                        peer,
                        ControlMessage::Map(MapControl::Clock {
                            counter: self.map.clock,
                        }),
                    );
                    return;
                }
                if !self.observe_control_clock(revision.counter) {
                    return;
                }
                self.stop_map_control();
                self.map.claim = revision;
            }
            MapControl::Prepare {
                epoch,
                revision,
                drag,
                side,
                fraction,
            } => {
                let accepted = self.can_receive_map(peer)
                    && (drag.is_none() || self.supports_drag(peer))
                    && epoch.generation > 0
                    && epoch.session == epoch.claim.counter
                    && self.map.pending.is_none()
                    && self
                        .map
                        .current
                        .as_ref()
                        .is_some_and(|m| m.revision == revision)
                    && epoch.claim == self.map.claim
                    && epoch.claim.author == peer
                    && fraction.is_finite()
                    && (0.0..1.0).contains(&fraction)
                    && self.map.epoch.is_none()
                    && self.map.highest.is_none_or(|e| {
                        e.claim < epoch.claim
                            || e.claim == epoch.claim && e.generation < epoch.generation
                            || self
                                .map
                                .prepared
                                .is_some_and(|(p, e, _, _, _)| p == peer && e == epoch)
                    })
                    && self
                        .map
                        .prepared
                        .as_ref()
                        .is_none_or(|(p, e, _, _, _)| *p == peer && *e == epoch);
                if accepted {
                    self.map.highest = Some(epoch);
                    self.map.prepared_drag = drag;
                    self.map.prepared = Some((peer, epoch, side, fraction, Instant::now()));
                }
                self.send_to(
                    peer,
                    ControlMessage::Map(MapControl::Prepared { epoch, accepted }),
                );
            }
            MapControl::Prepared { epoch, accepted } => self.map_prepared(peer, epoch, accepted),
            MapControl::Release { epoch }
                if self.map.epoch == Some(epoch)
                    && self.active_peer == Some(peer)
                    && self.session.is_controlled() =>
            {
                self.dispatch(SessionEvent::PeerReleased {
                    peer,
                    fraction: None,
                });
                self.map.epoch = None;
                self.map.released = Some(epoch);
                self.map.edge_since = None;
                self.send_to(peer, ControlMessage::Map(MapControl::Released { epoch }));
                self.start_transfer_on_grant(peer);
            }
            MapControl::Release { epoch }
                if self.map.released == Some(epoch) && epoch.claim.author == peer =>
            {
                self.send_to(peer, ControlMessage::Map(MapControl::Released { epoch }));
            }
            MapControl::Released { epoch }
                if self.map.epoch == Some(epoch)
                    && self
                        .map
                        .pending
                        .as_ref()
                        .is_some_and(|p| p.old == Some(peer) && p.stage == 1) =>
            {
                self.commit_map_transfer()
            }
            MapControl::Commit { epoch } => {
                if !self.can_receive_map(peer) {
                    self.stop_map_control();
                    return;
                }
                if self.map.epoch == Some(epoch)
                    && self.session.is_controlled()
                    && self.active_peer == Some(peer)
                {
                    self.send_to(peer, ControlMessage::Map(MapControl::Committed { epoch }));
                    return;
                }
                if let Some((owner, e, side, fraction, _)) =
                    self.map.prepared.filter(|(owner, e, _, _, since)| {
                        *owner == peer && *e == epoch && since.elapsed() < Duration::from_secs(1)
                    })
                {
                    if self.map.claim != epoch.claim {
                        return;
                    }
                    self.map.prepared = None;
                    self.incoming_drag = self
                        .map
                        .prepared_drag
                        .take()
                        .map(|drag| (peer, drag.transfer_id, false));
                    self.map.epoch = Some(e);
                    self.map.edge_since = None;
                    self.map.highest = Some(e);
                    self.active_peer = Some(owner);
                    self.session.activate_destination(
                        owner,
                        epoch.generation as u32,
                        side,
                        Instant::now(),
                    );
                    if let Some(link) = self.links.link_mut(peer) {
                        link.start_session(epoch.generation as u32);
                    }
                    self.apply(opendesk_core::session::SessionAction::WarpCursor {
                        side: side.opposite(),
                        fraction,
                    });
                    self.map.last_renew = Instant::now();
                    self.apply_strips();
                    self.send_to(peer, ControlMessage::Map(MapControl::Committed { epoch }));
                }
            }
            MapControl::Committed { epoch } => {
                if let Some(p) = self
                    .map
                    .pending
                    .clone()
                    .filter(|p| p.next == peer && p.epoch == epoch && p.stage == 2)
                {
                    self.map.pending = None;
                    self.map.epoch = Some(epoch);
                    self.active_peer = Some(peer);
                    self.session
                        .activate_origin(peer, epoch.generation as u32, p.side);
                    if let Some(link) = self.links.link_mut(peer) {
                        link.start_session(epoch.generation as u32);
                    }
                    self.map.last_renew = Instant::now();
                    self.start_transfer_on_grant(peer);
                    if std::mem::take(&mut self.map.drag_released) {
                        self.on_left_released(Instant::now());
                    }
                    for code in self.forwarded.modifiers() {
                        self.send_to(peer, self.key_message(peer, code, true));
                    }
                }
            }
            MapControl::ReturnDrag {
                epoch,
                side,
                fraction,
                drag,
            } if self.map.epoch == Some(epoch)
                && self.session.is_controlling()
                && self.active_peer == Some(peer)
                && self.supports_drag(peer) =>
            {
                if let Some(crossing) = self
                    .map_destination(peer, side, fraction)
                    .filter(|c| c.peer == self.identity.peer_id)
                {
                    self.authorize_return_drag(peer, drag);
                    self.begin_map_transfer(crossing.peer, side, crossing.fraction, Some(peer));
                }
            }
            MapControl::Edge {
                epoch,
                side,
                fraction,
            } if self.map.epoch == Some(epoch)
                && self.session.is_controlling()
                && self.active_peer == Some(peer) =>
            {
                if let Some(crossing) = self.map_destination(peer, side, fraction) {
                    self.begin_map_transfer(crossing.peer, side, crossing.fraction, Some(peer));
                }
            }
            MapControl::Renew { epoch }
                if self.map.epoch == Some(epoch)
                    && self.can_receive_map(peer)
                    && self.map.last_renew.elapsed() < Duration::from_secs(1)
                    && self.active_peer == Some(peer)
                    && self.session.is_controlled() =>
            {
                self.map.last_renew = Instant::now();
                self.send_to(peer, ControlMessage::Map(MapControl::Renewed { epoch }));
            }
            MapControl::Renewed { epoch }
                if self.map.epoch == Some(epoch)
                    && self.map.last_renew.elapsed() < Duration::from_secs(1)
                    && self.active_peer == Some(peer)
                    && self.session.is_controlling() =>
            {
                self.map.last_renew = Instant::now()
            }
            MapControl::Revoke { epoch }
                if (epoch.claim.author == peer || self.active_peer == Some(peer))
                    && (self.map.epoch == Some(epoch)
                        || self
                            .map
                            .prepared
                            .is_some_and(|(p, e, _, _, _)| p == peer && e == epoch)) =>
            {
                self.stop_map_control()
            }
            MapControl::Input { epoch, message }
                if self.map.epoch == Some(epoch)
                    && self.active_peer == Some(peer)
                    && self.map.last_renew.elapsed() < Duration::from_secs(1) =>
            {
                self.on_peer_input(peer, *message)
            }
            _ => {}
        }
    }
    pub(super) fn identify_local(&self) {
        let body = format!("{} · {}", self.identity.name, self.identity.peer_id);
        tokio::spawn(async move {
            let _ = super::super::notify::send_notification("Open Desktop", &body, 5000).await;
        });
    }
    pub(super) fn tick_map(&mut self, now: Instant) {
        if now.duration_since(self.map.last_status) >= Duration::from_millis(250) {
            self.map.last_status = now;
            self.broadcast(ControlMessage::Map(MapControl::Availability {
                input_status: if self.enabled {
                    self.input_status()
                } else {
                    "paused"
                }
                .into(),
                ready: self.enabled
                    && self.monitor.known(now)
                    && self.monitor.lock == super::monitor::LockState::Unlocked,
            }));
        }
        if self
            .map
            .pending
            .as_ref()
            .is_some_and(|p| now.duration_since(p.since) >= Duration::from_secs(1))
            || self.map.epoch.is_some()
                && now.duration_since(self.map.last_renew) >= Duration::from_secs(1)
        {
            self.stop_map_control();
        }
        if self
            .map
            .prepared
            .is_some_and(|(_, _, _, _, since)| now.duration_since(since) >= Duration::from_secs(1))
        {
            self.map.prepared = None;
        }
        if self.map.epoch.is_some()
            && (self.monitor.lock != super::monitor::LockState::Unlocked || !self.enabled)
        {
            self.stop_map_control();
        }
        if self.session.is_controlling()
            && now.duration_since(self.map.last_sent) >= Duration::from_millis(250)
        {
            self.map.last_sent = now;
            if let (Some(peer), Some(epoch)) = (self.active_peer, self.map.epoch) {
                self.send_to(peer, ControlMessage::Map(MapControl::Renew { epoch }));
            }
        }
    }
}

#[cfg(all(test, target_os = "linux"))]
mod tests;
