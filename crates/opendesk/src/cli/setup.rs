use std::io::{IsTerminal, Write};
use std::time::Duration;

use crate::daemon::ipc::{IpcRequest, IpcResponse, StatusReport};
use anyhow::{Context, bail};

fn prompt(message: &str) -> anyhow::Result<String> {
    print!("{message}");
    std::io::stdout().flush()?;
    let mut line = String::new();
    if std::io::stdin().read_line(&mut line)? == 0 {
        bail!("Entrada encerrada");
    }
    Ok(line.trim().to_owned())
}

async fn report() -> anyhow::Result<StatusReport> {
    match super::send(IpcRequest::Status).await? {
        IpcResponse::Status(report) => Ok(report),
        other => bail!("Resposta inesperada: {other:?}"),
    }
}

pub async fn run() -> anyhow::Result<()> {
    if !std::io::stdin().is_terminal() {
        bail!("Execute opendesk setup em um terminal, depois de concluir a instalação");
    }
    super::lifecycle::service("start")?;
    let mut ready = false;
    for _ in 0..50 {
        if report().await.is_ok() {
            ready = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    if !ready {
        bail!("Serviço não ficou pronto. Execute opendesk doctor");
    }
    println!("Open Desktop — configuração\nExecute setup também no outro computador.");
    println!("Em UM computador escolha iniciar; no OUTRO escolha receber.");
    let name = match prompt("[1] Iniciar pareamento  [2] Receber  [3] Configurar peer existente: ")?
        .as_str()
    {
        "1" => initiate().await?,
        "2" => receive().await?,
        "3" => {
            let current = report().await?;
            for peer in &current.peers {
                println!("  {}", peer.name);
            }
            prompt("Nome do computador: ")?
        }
        _ => bail!("Opção inválida; execute setup novamente"),
    };
    super::expect_ok(
        super::send(IpcRequest::PeerSet {
            name: name.clone(),
            side: "all".into(),
        })
        .await?,
    )?;
    println!("{name} configurado nas quatro bordas. Conclua setup na outra máquina também.");
    println!(
        "Empurre o cursor contra uma borda; retorne pela borda de entrada. Ctrl+Alt+Esc libera o controle."
    );
    super::status::run().await
}

async fn initiate() -> anyhow::Result<String> {
    println!("Buscando computadores na LAN…");
    tokio::time::sleep(Duration::from_secs(2)).await;
    match super::send(IpcRequest::Discover).await? {
        IpcResponse::Discovered(peers) => {
            if peers.is_empty() {
                bail!(
                    "Nenhum computador encontrado. Abra setup no outro host e confira opendesk doctor"
                );
            }
            for (index, peer) in peers.iter().enumerate() {
                println!(
                    "[{}] {} ({}) · {}",
                    index + 1,
                    peer.name,
                    peer.address,
                    peer.version
                );
            }
            let index: usize = prompt("Número do computador: ")?
                .parse()
                .context("Digite um número da lista")?;
            let peer = peers
                .get(index.wrapping_sub(1))
                .context("Número fora da lista")?;
            if !peer.paired {
                super::pair::run(peer.name.clone()).await?;
            }
            Ok(peer.name.clone())
        }
        other => bail!("Descoberta falhou: {other:?}"),
    }
}

async fn receive() -> anyhow::Result<String> {
    let original = report().await?;
    println!(
        "Aguardando por até 3 minutos. Inicie o pareamento no outro computador. Ctrl+C cancela esta espera."
    );
    let mut shown = None;
    for _ in 0..360 {
        let current = report().await?;
        if current.pending_pin != shown {
            if let Some(pin) = &current.pending_pin {
                println!("PIN para digitar no outro computador: {pin}");
            }
            shown = current.pending_pin.clone();
        }
        if let Some(peer) = current
            .peers
            .iter()
            .find(|peer| !original.peers.iter().any(|old| old.name == peer.name))
        {
            return Ok(peer.name.clone());
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
    bail!("Tempo esgotado. Se já pareou, execute setup e escolha configurar peer existente")
}
