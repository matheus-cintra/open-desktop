use std::collections::HashMap;

use anyhow::Context;
use zbus::zvariant::Value;
use zbus::{Connection, Proxy};

const APP_NAME: &str = "opendesk";
const APP_ICON: &str = "input-mouse";
const SERVICE: &str = "org.freedesktop.Notifications";
const PATH: &str = "/org/freedesktop/Notifications";
const INTERFACE: &str = "org.freedesktop.Notifications";
const METHOD: &str = "Notify";

pub async fn send_notification(summary: &str, body: &str, timeout_ms: i32) -> anyhow::Result<u32> {
    let connection = Connection::session()
        .await
        .context("failed to connect to the session bus")?;
    let notifications = Proxy::new(&connection, SERVICE, PATH, INTERFACE)
        .await
        .context("failed to create the notifications proxy")?;
    let replaces_id: u32 = 0;
    let actions: &[&str] = &[];
    let hints: HashMap<&str, Value<'_>> = HashMap::new();
    let id: u32 = notifications
        .call(
            METHOD,
            &(
                APP_NAME,
                replaces_id,
                APP_ICON,
                summary,
                body,
                actions,
                hints,
                timeout_ms,
            ),
        )
        .await
        .context("org.freedesktop.Notifications.Notify failed")?;
    tracing::debug!(id, summary, "desktop notification sent");
    Ok(id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    #[ignore = "needs a running notification daemon on the session bus"]
    async fn sends_a_real_notification() {
        let id = send_notification("opendesk test", "notification from cargo test", 5000)
            .await
            .unwrap();
        println!("notification id: {id}");
        assert!(id > 0);
    }
}
