use clap::Parser;
use opendesk::cli::{Cli, Command};

#[test]
fn lifecycle_commands_parse_without_daemon() {
    for (argument, expected) in [
        ("setup", Command::Setup),
        ("install", Command::Install),
        ("start", Command::Start),
        ("stop", Command::Stop),
        ("restart", Command::Restart),
        ("doctor", Command::Doctor),
        ("logs", Command::Logs),
        ("uninstall", Command::Uninstall),
        ("pause", Command::Disable),
        ("disable", Command::Disable),
        ("resume", Command::Enable),
        ("enable", Command::Enable),
    ] {
        let parsed = Cli::try_parse_from(["opendesk", argument]);
        assert!(
            matches!(parsed, Ok(cli) if cli.command == expected),
            "{argument}"
        );
    }
}

#[test]
fn update_accepts_explicit_version_or_latest_default() {
    assert!(matches!(
        Cli::try_parse_from(["opendesk", "update"]),
        Ok(Cli {
            command: Command::Update { version: None }
        })
    ));
    assert!(
        matches!(Cli::try_parse_from(["opendesk", "update", "v0.1.0"]),
        Ok(Cli { command: Command::Update { version: Some(version) } }) if version == "v0.1.0")
    );
}
