use std::path::PathBuf;
use std::process::Command;

use anyhow::{Context, bail};

const INSTALLER: &str = include_str!("../../../../scripts/install-user.sh");
const BOOTSTRAP: &str = include_str!("../../../../install.sh");
const UNIT: &str = include_str!("../../../../packaging/opendesk.service");

struct Stage(PathBuf);
impl Stage {
    fn new() -> anyhow::Result<Self> {
        let output = Command::new("mktemp").args(["-d"]).output()?;
        if !output.status.success() {
            bail!("Não foi possível criar o diretório temporário");
        }
        Ok(Self(PathBuf::from(
            String::from_utf8(output.stdout)?.trim(),
        )))
    }
}
impl Drop for Stage {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn checked(command: &mut Command) -> anyhow::Result<()> {
    let status = command
        .status()
        .context("Falha ao executar comando do sistema")?;
    if !status.success() {
        bail!("Comando terminou com {status}");
    }
    Ok(())
}

pub fn install(action: &str) -> anyhow::Result<()> {
    let stage = Stage::new()?;
    std::fs::create_dir(stage.0.join("scripts"))?;
    std::fs::create_dir(stage.0.join("packaging"))?;
    let script = stage.0.join("scripts/install-user.sh");
    std::fs::write(&script, INSTALLER)?;
    std::fs::write(stage.0.join("packaging/opendesk.service"), UNIT)?;
    checked(
        Command::new("bash")
            .arg(script)
            .arg(action)
            .env("OPENDESK_BIN", std::env::current_exe()?),
    )
}

pub fn service(action: &str) -> anyhow::Result<()> {
    checked(Command::new("systemctl").args(["--user", action, "opendesk.service"]))
}

pub fn logs() -> anyhow::Result<()> {
    checked(Command::new("journalctl").args(["--user", "-u", "opendesk.service", "-n", "50", "-f"]))
}

pub fn update(version: Option<&str>) -> anyhow::Result<()> {
    let stage = Stage::new()?;
    let script = stage.0.join("install.sh");
    std::fs::write(&script, BOOTSTRAP)?;
    checked(
        Command::new("sh")
            .arg(script)
            .arg(version.unwrap_or("latest"))
            .env("OPENDESK_NO_SETUP", "1"),
    )
}

pub async fn doctor() -> anyhow::Result<()> {
    println!(
        "Open Desktop {} · Linux x86_64 · protocolo 2",
        env!("CARGO_PKG_VERSION")
    );
    let mut failures = 0;
    for (label, program, args) in [
        (
            "Serviço instalado e habilitado",
            "systemctl",
            vec!["--user", "is-enabled", "opendesk.service"],
        ),
        (
            "Serviço ativo",
            "systemctl",
            vec!["--user", "is-active", "opendesk.service"],
        ),
        (
            "Sessão gráfica UWSM",
            "systemctl",
            vec!["--user", "is-active", "graphical-session.target"],
        ),
        (
            "Compositor",
            "systemd-run",
            vec![
                "--user",
                "--quiet",
                "--wait",
                "--pipe",
                "--collect",
                "hyprctl",
                "version",
            ],
        ),
    ] {
        match Command::new(program).args(args).output() {
            Ok(output) if output.status.success() => {
                println!(
                    "✓ {label}: {}",
                    String::from_utf8_lossy(&output.stdout).trim()
                );
            }
            _ => {
                println!("✗ {label}");
                failures += 1;
            }
        }
    }
    match Command::new("systemd-run")
        .args([
            "--user",
            "--quiet",
            "--wait",
            "--pipe",
            "--collect",
            "hyprctl",
            "configerrors",
        ])
        .output()
    {
        Ok(output)
            if output.status.success() && output.stdout.iter().all(u8::is_ascii_whitespace) =>
        {
            println!("✓ Configuração Hyprland sem erros");
        }
        _ => {
            println!("✗ Configuração Hyprland: execute hyprctl configerrors");
            failures += 1;
        }
    }
    match super::send(crate::daemon::ipc::IpcRequest::Status).await {
        Ok(crate::daemon::ipc::IpcResponse::Status(report)) => {
            println!("✓ IPC acessível\n{}", super::status::render(&report));
        }
        other => {
            println!("✗ IPC: {other:?}");
            failures += 1;
        }
    }
    println!("Rede: permita TCP/UDP 47820 e mDNS UDP 5353 somente na LAN confiável.");
    println!("Use opendesk discover para verificar descoberta; opendesk logs para diagnóstico.");
    if failures > 0 {
        bail!("{failures} verificações falharam");
    }
    Ok(())
}
