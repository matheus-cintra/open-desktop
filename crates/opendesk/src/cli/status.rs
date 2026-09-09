use rust_i18n::t;

use super::{CliError, send, yes_no};
use crate::daemon::ipc::{IpcRequest, IpcResponse, StatusReport};

pub async fn run() -> anyhow::Result<()> {
    match send(IpcRequest::Status).await? {
        IpcResponse::Status(report) => {
            print!("{}", render(&report));
            Ok(())
        }
        IpcResponse::Error { message } => Err(CliError::Daemon(message).into()),
        _ => Err(CliError::UnexpectedResponse.into()),
    }
}

pub fn render(report: &StatusReport) -> String {
    let unknown = t!("common.unknown").into_owned();
    let mut lines = vec![
        t!("status.name", name = report.name).into_owned(),
        t!("status.peer_id", peer_id = report.peer_id).into_owned(),
        t!("status.state", state = report.state).into_owned(),
        t!("status.enabled", enabled = yes_no(report.enabled)).into_owned(),
    ];
    if report.peers.is_empty() {
        lines.push(t!("status.no_peers").into_owned());
    } else {
        lines.push(t!("status.peers_header").into_owned());
        for peer in &report.peers {
            lines.push(
                t!(
                    "status.peer_line",
                    name = peer.name,
                    side = peer.side.as_deref().unwrap_or(&unknown),
                    connected = yes_no(peer.connected),
                    address = peer.address.as_deref().unwrap_or(&unknown)
                )
                .into_owned(),
            );
        }
    }
    if let Some(pin) = &report.pending_pin {
        lines.push(t!("status.pending_pin", pin = pin).into_owned());
    }
    lines.push(String::new());
    lines.join("\n")
}
