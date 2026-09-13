pub async fn send_notification(summary: &str, body: &str, _: i32) -> anyhow::Result<u32> {
    tracing::info!(
        summary,
        body,
        "notification (also available in menu status)"
    );
    Ok(0)
}
