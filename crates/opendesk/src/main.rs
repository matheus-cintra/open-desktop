use std::process::ExitCode;

#[cfg(target_os = "linux")]
use clap::Parser;
#[cfg(target_os = "linux")]
use tracing_subscriber::EnvFilter;

#[cfg(target_os = "linux")]
#[tokio::main]
async fn main() -> ExitCode {
    opendesk::i18n::init_locale_from_env();
    init_tracing();
    let arguments = opendesk::cli::Cli::parse();
    match opendesk::cli::run(arguments.command).await {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{}", opendesk::cli::render_error(&error));
            ExitCode::FAILURE
        }
    }
}

#[cfg(target_os = "linux")]
fn init_tracing() {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    let builder = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(std::io::stderr);
    if std::env::var_os("JOURNAL_STREAM").is_some() {
        builder.without_time().init();
    } else {
        builder.init();
    }
}

#[cfg(target_os = "macos")]
fn main() -> ExitCode {
    opendesk::macos_app::main()
}
