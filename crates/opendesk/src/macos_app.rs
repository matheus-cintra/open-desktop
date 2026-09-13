use crate::daemon::ipc::{IpcRequest, IpcResponse};
use clap::Parser;
use std::process::ExitCode;

pub fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().collect();
    if args.get(1).map(String::as_str) == Some("--opendesk-permission-probe") && args.len() == 2 {
        return ExitCode::from(opendesk_macos::permission_probe());
    }
    if args.get(1).map(String::as_str) == Some("--opendesk-permission-relaunch") && args.len() == 3
    {
        return ExitCode::from(
            args[2]
                .parse()
                .map(opendesk_macos::permission_relaunch)
                .unwrap_or(1),
        );
    }
    crate::i18n::init_locale_from_env();
    let app = std::env::args().len() == 1 || std::env::args().nth(1).as_deref() == Some("daemon");
    let runtime = match tokio::runtime::Runtime::new() {
        Ok(runtime) => runtime,
        Err(error) => {
            eprintln!("{error}");
            return ExitCode::FAILURE;
        }
    };
    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"));
    if app {
        let home = std::env::var_os("HOME").unwrap_or_default();
        let logs = std::path::PathBuf::from(home).join("Library/Logs/Open Desktop");
        if let Err(error) = std::fs::create_dir_all(&logs) {
            eprintln!("{error}");
            return ExitCode::FAILURE;
        }
        let file = match std::fs::OpenOptions::new()
            .append(true)
            .create(true)
            .open(logs.join("app.log"))
        {
            Ok(file) => file,
            Err(error) => {
                eprintln!("{error}");
                return ExitCode::FAILURE;
            }
        };
        tracing_subscriber::fmt()
            .with_env_filter(filter)
            .with_ansi(false)
            .with_writer(std::sync::Mutex::new(file))
            .init();
        // Finder and login launch the same app; refuse to replace an existing IPC owner.
        if runtime
            .block_on(crate::daemon::ipc::client::request(
                &match crate::daemon::ipc::socket_path() {
                    Ok(path) => path,
                    Err(_) => return ExitCode::FAILURE,
                },
                IpcRequest::Status,
            ))
            .is_ok()
        {
            return ExitCode::SUCCESS;
        }
        runtime.spawn(async {
            if let Err(error) = crate::daemon::run().await {
                tracing::error!(%error, "engine stopped");
            }
            opendesk_macos::quit();
        });
        let handle = runtime.handle().clone();
        opendesk_macos::run_app(move |text| {
            let result = handle.block_on(async {
                let request = serde_json::from_str::<IpcRequest>(text)?;
                let path = crate::daemon::ipc::socket_path()?;
                tokio::time::timeout(
                    std::time::Duration::from_secs(15),
                    crate::daemon::ipc::client::request(&path, request),
                )
                .await?
            });
            let reply = result.unwrap_or_else(|error| IpcResponse::Error {
                message: error.to_string(),
            });
            serde_json::to_string(&reply).unwrap_or_else(|_| "null".into())
        });
        return ExitCode::SUCCESS;
    }
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(std::io::stderr)
        .init();
    match runtime.block_on(crate::cli::run(crate::cli::Cli::parse().command)) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{}", crate::cli::render_error(&error));
            ExitCode::FAILURE
        }
    }
}
