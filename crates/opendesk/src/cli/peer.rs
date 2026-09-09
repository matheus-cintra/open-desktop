use super::{PeerAction, expect_ok, send};
use crate::daemon::ipc::IpcRequest;

pub async fn run(action: PeerAction) -> anyhow::Result<()> {
    let request = match action {
        PeerAction::Set { name, side } => IpcRequest::PeerSet { name, side },
        PeerAction::Remove { name } => IpcRequest::PeerRemove { name },
    };
    expect_ok(send(request).await?)
}
