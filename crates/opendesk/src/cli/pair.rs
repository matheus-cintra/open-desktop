use rust_i18n::t;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

use super::{CliError, expect_ok, send};
use crate::daemon::ipc::{IpcRequest, IpcResponse};

pub async fn run(name: String) -> anyhow::Result<()> {
    match send(IpcRequest::Pair { name: name.clone() }).await? {
        IpcResponse::PinRequired => {
            let pin = prompt_pin(&name).await?;
            expect_ok(send(IpcRequest::SubmitPin { pin }).await?)
        }
        other => expect_ok(other),
    }
}

async fn prompt_pin(name: &str) -> anyhow::Result<String> {
    let mut stdout = tokio::io::stdout();
    stdout
        .write_all(t!("pair.pin_prompt", name = name).as_bytes())
        .await?;
    stdout.flush().await?;
    let mut line = String::new();
    BufReader::new(tokio::io::stdin())
        .read_line(&mut line)
        .await?;
    let pin = line.trim().to_owned();
    if pin.is_empty() {
        return Err(CliError::Daemon(t!("pair.pin_empty").into_owned()).into());
    }
    Ok(pin)
}
