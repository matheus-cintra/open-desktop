use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::time::Instant;

use opendesk_core::session::SessionEvent;
use opendesk_proto::control::{ControlMessage, PeerId, Side};
use opendesk_wayland::WaylandCommand;
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
            || !self.session.can_return_at(side, Instant::now())
        {
            return;
        }
        let Some(peer) = self.active_peer else {
            return;
        };
        let transfer_id = self.next_transfer_id;
        self.next_transfer_id += 1;
        match plan_transfer(&uris, transfer_id) {
            Ok(plan) => {
                if generation != self.sinks.wayland.drag_generation.load(Ordering::Acquire) {
                    return;
                }
                self.send_to(
                    peer,
                    ControlMessage::ReturnDrag {
                        drag: plan.drag.clone(),
                    },
                );
                self.wayland(WaylandCommand::AbortLocalDrag);
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
