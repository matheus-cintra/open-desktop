use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::time::Instant;

use crate::platform::PlatformCommand;
use opendesk_core::session::SessionEvent;
use opendesk_proto::control::{ControlMessage, PeerId, Side};
use tracing::warn;

use super::Engine;
use crate::daemon::net::tcp::TcpCommand;
use crate::daemon::transfer::{TransferPlan, plan_transfer, run_transfer};

impl Engine {
    pub(super) fn return_drag_crossing(
        &mut self,
        generation: u64,
        side: Side,
        fraction: f32,
        uris: Vec<PathBuf>,
    ) {
        if generation != self.sinks.wayland.drag_generation.load(Ordering::Acquire)
            || (self.map.epoch.is_none() && !self.session.can_return_at(side, Instant::now()))
        {
            return;
        }
        let Some(peer) = self.active_peer else {
            return;
        };
        if self.map.epoch.is_some()
            && self
                .map_destination(self.identity.peer_id, side, fraction)
                .is_none_or(|c| c.peer != peer)
        {
            return;
        }
        if !self.supports_drag(peer) {
            return;
        }
        let transfer_id = self.next_transfer_id;
        self.next_transfer_id += 1;
        match plan_transfer(&uris, transfer_id) {
            Ok(plan) => {
                if generation != self.sinks.wayland.drag_generation.load(Ordering::Acquire) {
                    return;
                }
                if let Some(epoch) = self.map.epoch {
                    self.send_to(
                        peer,
                        ControlMessage::Map(opendesk_proto::map::MapControl::ReturnDrag {
                            epoch,
                            side,
                            fraction,
                            drag: plan.drag.clone(),
                        }),
                    );
                    self.pending_transfer = Some(plan);
                    self.wayland(PlatformCommand::AbortLocalDrag);
                    return;
                }
                self.send_to(
                    peer,
                    ControlMessage::ReturnDrag {
                        drag: plan.drag.clone(),
                    },
                );
                self.wayland(PlatformCommand::AbortLocalDrag);
                self.dispatch(SessionEvent::EdgeEntered {
                    side,
                    fraction,
                    peer: None,
                });
                self.start_return_transfer(peer, plan);
            }
            Err(error) => warn!(%error, "could not plan the returning file transfer"),
        }
    }

    fn start_return_transfer(&self, peer: PeerId, plan: TransferPlan) {
        let Some(link) = self.links.link(peer) else {
            return;
        };
        let connection = link.connection;
        let tcp = self.sinks.tcp.clone();
        tokio::spawn(async move {
            let transfer_id = plan.drag.transfer_id;
            if let Err(error) = run_transfer(plan, connection, tcp.clone()).await {
                warn!(%error, "returning file transfer failed");
                let _ = tcp.send(TcpCommand::Send {
                    connection,
                    message: ControlMessage::DragCancel { transfer_id },
                });
            }
        });
    }
}
