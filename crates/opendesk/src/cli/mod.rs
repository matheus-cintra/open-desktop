mod lifecycle;
pub mod pair;
pub mod peer;
mod setup;
pub mod simple;
pub mod status;

use clap::{Parser, Subcommand};
use rust_i18n::t;

use crate::daemon::ipc::client::{IpcClientError, request};
use crate::daemon::ipc::{IpcRequest, IpcResponse, socket_path};

#[derive(Parser, Debug)]
#[command(
    name = "opendesk",
    version,
    about = "Cross-machine cursor, keyboard, clipboard and file drag for Hyprland"
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand, Debug, Clone, PartialEq, Eq)]
pub enum Command {
    #[command(about = "Run the daemon in the foreground")]
    Daemon,
    /// Configure this computer and pair interactively
    Setup,
    /// Install this binary and the Hyprland integration for your user
    Install,
    /// Start the background service
    Start,
    /// Stop the background service
    Stop,
    /// Restart the background service
    Restart,
    /// Check installation and graphical session
    Doctor,
    /// Follow service logs (Ctrl+C to exit)
    Logs,
    /// Install the latest release, or a specific version
    Update { version: Option<String> },
    /// Uninstall; preserve identity, pairing and configuration
    Uninstall,
    #[command(about = "Show the daemon state and the configured peers")]
    Status,
    #[command(about = "List opendesk peers found on the local network")]
    Discover,
    #[command(about = "Pair with a discovered peer using a PIN")]
    Pair { name: String },
    #[command(about = "Manage paired peers")]
    Peer {
        #[command(subcommand)]
        action: PeerAction,
    },
    #[command(about = "Release control and bring the cursor back")]
    Release,
    #[command(name = "resume", alias = "enable", about = "Enable edge crossing")]
    Enable,
    #[command(name = "pause", alias = "disable", about = "Disable edge crossing")]
    Disable,
}

#[derive(Subcommand, Debug, Clone, PartialEq, Eq)]
pub enum PeerAction {
    #[command(about = "Place a paired peer on a side: left, right, top, bottom or all")]
    Set {
        name: String,
        #[arg(value_name = "SIDE")]
        side: Option<String>,
        #[arg(long = "side", value_name = "SIDE")]
        side_option: Option<String>,
    },
    #[command(about = "Forget a paired peer")]
    Remove { name: String },
}

#[derive(Debug, thiserror::Error)]
pub enum CliError {
    #[error("daemon returned an error: {0}")]
    Daemon(String),
    #[error("unexpected response from the daemon")]
    UnexpectedResponse,
}

pub async fn run(command: Command) -> anyhow::Result<()> {
    match command {
        Command::Setup => setup::run().await,
        Command::Install => lifecycle::install("install"),
        Command::Uninstall => lifecycle::install("uninstall"),
        Command::Start => lifecycle::service("start"),
        Command::Stop => lifecycle::service("stop"),
        Command::Restart => lifecycle::service("restart"),
        Command::Doctor => lifecycle::doctor().await,
        Command::Logs => lifecycle::logs(),
        Command::Update { version } => lifecycle::update(version.as_deref()),
        Command::Daemon => crate::daemon::run().await,
        Command::Status => status::run().await,
        Command::Discover => simple::discover().await,
        Command::Pair { name } => pair::run(name).await,
        Command::Peer { action } => peer::run(action).await,
        Command::Release => simple::send_and_expect_ok(IpcRequest::Release).await,
        Command::Enable => simple::send_and_expect_ok(IpcRequest::Enable).await,
        Command::Disable => simple::send_and_expect_ok(IpcRequest::Disable).await,
    }
}

pub fn render_error(error: &anyhow::Error) -> String {
    if let Some(IpcClientError::DaemonNotRunning(_)) = error.downcast_ref::<IpcClientError>() {
        return t!("cli.daemon_not_running").into_owned();
    }
    match error.downcast_ref::<CliError>() {
        Some(CliError::Daemon(message)) => t!("cli.error", message = message).into_owned(),
        Some(CliError::UnexpectedResponse) => t!("cli.unexpected_response").into_owned(),
        None => t!("cli.failed", error = format!("{error:#}")).into_owned(),
    }
}

async fn send(request_body: IpcRequest) -> anyhow::Result<IpcResponse> {
    let path = socket_path()?;
    request(&path, request_body).await
}

fn expect_ok(response: IpcResponse) -> anyhow::Result<()> {
    match response {
        IpcResponse::Ok => {
            println!("{}", t!("cli.ok"));
            Ok(())
        }
        IpcResponse::Error { message } => Err(CliError::Daemon(message).into()),
        _ => Err(CliError::UnexpectedResponse.into()),
    }
}

fn yes_no(value: bool) -> String {
    if value {
        t!("common.yes").into_owned()
    } else {
        t!("common.no").into_owned()
    }
}
