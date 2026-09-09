use rust_i18n::t;

use super::{CliError, expect_ok, send, yes_no};
use crate::daemon::ipc::{DiscoveredPeerReport, IpcRequest, IpcResponse};

pub async fn send_and_expect_ok(request: IpcRequest) -> anyhow::Result<()> {
    expect_ok(send(request).await?)
}

pub async fn discover() -> anyhow::Result<()> {
    match send(IpcRequest::Discover).await? {
        IpcResponse::Discovered(peers) => {
            print!("{}", render_discovered(&peers));
            Ok(())
        }
        IpcResponse::Error { message } => Err(CliError::Daemon(message).into()),
        _ => Err(CliError::UnexpectedResponse.into()),
    }
}

pub fn render_discovered(peers: &[DiscoveredPeerReport]) -> String {
    if peers.is_empty() {
        return format!("{}\n", t!("discover.none"));
    }
    peers
        .iter()
        .map(|peer| {
            format!(
                "{}\n",
                t!(
                    "discover.line",
                    name = peer.name,
                    peer_id = peer.peer_id,
                    address = peer.address,
                    version = peer.version,
                    paired = yes_no(peer.paired)
                )
            )
        })
        .collect()
}
