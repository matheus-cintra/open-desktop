#[cfg(test)]
mod common;

use std::net::SocketAddr;

use opendesk_core::config::{Config, ConfigError, PeerConfig};
use opendesk_proto::control::Side;

use common::TempDir;

#[test]
fn defaults_match_the_plan() {
    let config = Config::default();
    let general = &config.general;
    assert!(!general.name.is_empty());
    assert_eq!(general.port, opendesk_proto::DEFAULT_PORT);
    assert_eq!(general.edge_threshold_px, 60.0);
    assert_eq!(general.edge_cancel_px, 8.0);
    assert_eq!(general.motion_rate_hz, 0);
    assert_eq!(general.release_hotkey.to_string(), "ctrl+alt+escape");
    assert_eq!(general.clipboard_max_bytes, 10 * 1024 * 1024);
    assert!(general.dnd_dir.ends_with(".cache/opendesk/dnd"));
    assert!(general.dnd_dir.is_absolute());
    assert_eq!(general.dnd_timeout_s, 120);
    assert_eq!(general.dnd_keep_days, 7);
    assert_eq!(general.bar_color.to_string(), "#5e81accc");
    assert!(config.peers.is_empty());
}

#[test]
fn missing_file_loads_defaults() {
    let directory = TempDir::new("config-missing");
    let loaded = Config::load(&directory.path().join("config.toml")).unwrap();
    assert_eq!(loaded, Config::default());
}

#[test]
fn parses_the_documented_toml_shape() {
    let text = r##"
[general]
name = "desktop"
port = 47821
edge_threshold_px = 80
release_hotkey = "super+escape"
dnd_dir = "~/dnd"
bar_color = "#ff000080"

[[peer]]
name = "notebook"
side = "left"
addr = "192.168.15.20:47820"

[[peer]]
name = "tablet"
side = "top"
"##;
    let directory = TempDir::new("config-parse");
    let path = directory.path().join("config.toml");
    std::fs::write(&path, text).unwrap();
    let config = Config::load(&path).unwrap();
    assert_eq!(config.general.name, "desktop");
    assert_eq!(config.general.port, 47821);
    assert_eq!(config.general.edge_threshold_px, 80.0);
    assert_eq!(config.general.edge_cancel_px, 8.0);
    assert_eq!(config.general.release_hotkey.to_string(), "super+escape");
    assert_eq!(
        config.general.dnd_dir,
        dirs::home_dir().unwrap().join("dnd")
    );
    assert_eq!(config.general.bar_color.to_string(), "#ff000080");
    assert_eq!(
        config.peers,
        vec![
            PeerConfig {
                name: "notebook".to_owned(),
                side: Side::Left,
                addr: Some("192.168.15.20:47820".parse::<SocketAddr>().unwrap()),
            },
            PeerConfig {
                name: "tablet".to_owned(),
                side: Side::Top,
                addr: None,
            },
        ]
    );
}

#[test]
fn parse_errors_carry_the_path() {
    let directory = TempDir::new("config-error");
    let path = directory.path().join("config.toml");
    std::fs::write(&path, "[[peer]]\nname = \"x\"\nside = \"diagonal\"\n").unwrap();
    let result = Config::load(&path);
    assert!(
        matches!(result, Err(ConfigError::Parse { .. })),
        "{result:?}"
    );
    if let Err(ConfigError::Parse {
        path: reported,
        message,
    }) = result
    {
        assert_eq!(reported, path);
        assert!(message.contains("diagonal"), "{message}");
    }
}

#[test]
fn save_and_load_round_trip() {
    let directory = TempDir::new("config-roundtrip");
    let path = directory.path().join("nested").join("config.toml");
    let mut config = Config::default();
    config.general.name = "desktop".to_owned();
    config.general.release_hotkey = "ctrl+shift+f12".parse().unwrap();
    config.general.bar_color = "#123456".parse().unwrap();
    config.peers.push(PeerConfig {
        name: "notebook".to_owned(),
        side: Side::Right,
        addr: Some("10.0.0.2:47820".parse().unwrap()),
    });
    config.save(&path).unwrap();
    let text = std::fs::read_to_string(&path).unwrap();
    assert!(text.contains("[[peer]]"), "{text}");
    assert!(text.contains("side = \"right\""), "{text}");
    assert!(
        text.contains("dnd_dir = \"~/.cache/opendesk/dnd\""),
        "{text}"
    );
    assert_eq!(Config::load(&path).unwrap(), config);
}

#[test]
fn peer_lookups_by_side_and_name() {
    let mut config = Config::default();
    config.set_peer_side("notebook", Side::Left);
    assert_eq!(
        config
            .peer_for_side(Side::Left)
            .map(|peer| peer.name.as_str()),
        Some("notebook")
    );
    assert_eq!(config.side_for_peer("notebook"), Some(Side::Left));
    assert_eq!(config.side_for_peer("tablet"), None);
    assert!(config.peer_for_side(Side::Right).is_none());
}

#[test]
fn set_peer_side_moves_a_side_between_peers() {
    let mut config = Config::default();
    config.set_peer_side("notebook", Side::Left);
    config.set_peer_side("tablet", Side::Top);
    config.set_peer_side("tablet", Side::Left);
    assert_eq!(config.side_for_peer("tablet"), Some(Side::Left));
    assert_eq!(config.side_for_peer("notebook"), None);
    assert_eq!(config.peers.len(), 1);
    config.set_peer_side("tablet", Side::Left);
    assert_eq!(config.peers.len(), 1);
}

#[test]
fn remove_peer_reports_whether_it_existed() {
    let mut config = Config::default();
    config.set_peer_side("notebook", Side::Left);
    assert!(config.remove_peer("notebook"));
    assert!(!config.remove_peer("notebook"));
    assert!(config.peers.is_empty());
}

#[test]
fn default_path_honours_the_environment_override() {
    let path = Config::default_path().unwrap();
    assert!(
        path.ends_with("opendesk/config.toml") || std::env::var_os("OPENDESK_CONFIG").is_some()
    );
}
