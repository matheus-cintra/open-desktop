#[cfg(target_os = "linux")]
pub(super) fn spawn() -> tokio::sync::mpsc::Receiver<()> {
    use tokio::{io::AsyncReadExt, net::UnixStream};
    let (tx, rx) = tokio::sync::mpsc::channel(1);
    tokio::spawn(async move {
        loop {
            if let Ok(mut stream) =
                UnixStream::connect("/run/opendesk-activity/activity.sock").await
            {
                // The socket must be owned by the dedicated system user, never
                // an arbitrary process in the desktop user's runtime directory.
                let service_uid = std::fs::read_to_string("/etc/passwd")
                    .ok()
                    .and_then(|text| {
                        text.lines()
                            .find(|line| line.starts_with("opendesk-activity:"))
                            .and_then(|line| line.split(':').nth(2))
                            .and_then(|uid| uid.parse::<u32>().ok())
                    });
                let trusted = stream
                    .peer_cred()
                    .ok()
                    .is_some_and(|c| Some(c.uid()) == service_uid);
                if trusted {
                    let mut byte = [0];
                    while stream.read_exact(&mut byte).await.is_ok() {
                        if byte[0] == b'A' {
                            let _ = tx.try_send(());
                        }
                    }
                }
            }
            tokio::time::sleep(std::time::Duration::from_secs(1)).await;
        }
    });
    rx
}
#[cfg(target_os = "macos")]
pub(super) fn spawn() -> tokio::sync::mpsc::Receiver<()> {
    let (_, rx) = tokio::sync::mpsc::channel(1);
    rx
}
