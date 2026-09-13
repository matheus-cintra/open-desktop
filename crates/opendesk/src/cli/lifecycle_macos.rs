use anyhow::{Context, bail};
use std::path::PathBuf;
use std::process::Command;
fn bundle() -> anyhow::Result<PathBuf> {
    Ok(
        PathBuf::from(std::env::var_os("HOME").context("HOME not set")?)
            .join("Applications/Open Desktop.app"),
    )
}
fn checked(command: &mut Command) -> anyhow::Result<()> {
    anyhow::ensure!(command.status()?.success(), "macOS command failed");
    Ok(())
}
pub fn service(action: &str) -> anyhow::Result<()> {
    match action {
        "start" => checked(Command::new("/usr/bin/open").arg(bundle()?)),
        "stop" | "restart" => {
            checked(
                Command::new("/usr/bin/osascript")
                    .args(["-e", "tell application id \"dev.mcintra.opendesk\" to quit"]),
            )?;
            if action == "restart" {
                service("start")?;
            }
            Ok(())
        }
        _ => bail!("Unsupported service action: {action}"),
    }
}
pub fn install(action: &str) -> anyhow::Result<()> {
    if action == "uninstall" {
        service("stop")?;
        std::fs::remove_dir_all(bundle()?)?;
        return Ok(());
    }
    bail!("Install the ARM64 app using scripts/install-macos.sh from the source checkout")
}
pub fn update(_: Option<&str>) -> anyhow::Result<()> {
    bail!(
        "This is a local macOS preview. Build and install the matching Linux/macOS checkout; public updates are not available yet"
    )
}
pub fn logs() -> anyhow::Result<()> {
    let log = PathBuf::from(std::env::var_os("HOME").context("HOME not set")?)
        .join("Library/Logs/Open Desktop/app.log");
    checked(
        Command::new("/usr/bin/tail")
            .args(["-n", "50", "-f"])
            .arg(log),
    )
}
pub async fn doctor() -> anyhow::Result<()> {
    println!(
        "Open Desktop {} · macOS arm64 · protocolo {}",
        env!("CARGO_PKG_VERSION"),
        opendesk_proto::PROTOCOL_VERSION
    );
    println!("App: {}", bundle()?.display());
    super::status::run().await?;
    println!(
        "Confira permissões e estado de captura no menu do app. Um monitor ativo; LAN TCP/UDP 47820 e mDNS 5353."
    );
    Ok(())
}
