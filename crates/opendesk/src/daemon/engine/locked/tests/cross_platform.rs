use super::*;
use opendesk_proto::keyboard::{evdev_to_hid, swap_control_super};

#[test]
fn mac_peer_uses_hid_and_swaps_control_on_receive() {
    let mut f = Fixture::new();
    f.engine.set_capabilities(PEER, true, false);
    assert!(f.engine.cross_platform(PEER));
    assert!(!f.engine.supports_drag(PEER));
    assert_eq!(
        f.engine.key_message(PEER, 29, true),
        ControlMessage::PhysicalKey {
            usage: evdev_to_hid(29).unwrap(),
            pressed: true,
        }
    );
    f.receive(Side::Left, false);
    f.engine.on_peer_input(
        PEER,
        ControlMessage::PhysicalKey {
            usage: 227,
            pressed: true,
        },
    );
    f.engine.on_peer_input(
        PEER,
        ControlMessage::PhysicalKey {
            usage: 227,
            pressed: false,
        },
    );
    let commands = recovery::commands(&f);
    assert!(commands.contains(&PlatformCommand::InjectPhysicalKey {
        code: 29,
        pressed: true
    }));
    assert!(commands.contains(&PlatformCommand::InjectPhysicalKey {
        code: 29,
        pressed: false
    }));
    assert_eq!(swap_control_super(227), 224);
}

#[test]
fn linux_peer_retains_xkb_key_representation() {
    let f = Fixture::new();
    assert!(!f.engine.cross_platform(PEER));
    assert!(f.engine.supports_drag(PEER));
    assert_eq!(
        f.engine.key_message(PEER, 29, true),
        ControlMessage::Key {
            code: 29,
            pressed: true
        }
    );
}

#[test]
fn mac_peer_cannot_inject_raw_xkb_or_unknown_hid() {
    let mut f = Fixture::new();
    f.engine.set_capabilities(PEER, true, false);
    f.receive(Side::Left, false);
    let _ = recovery::commands(&f);
    for message in [
        ControlMessage::PhysicalKey {
            usage: 65535,
            pressed: true,
        },
        ControlMessage::Key {
            code: 29,
            pressed: true,
        },
        ControlMessage::Modifiers {
            depressed: 255,
            latched: 0,
            locked: 0,
            group: 0,
        },
    ] {
        f.engine.on_peer_input(PEER, message);
    }
    assert!(recovery::commands(&f).is_empty());
    assert!(f.engine.injected.is_empty());
}

#[test]
fn recovery_releases_translated_keys_with_physical_backend() {
    let mut f = Fixture::new();
    f.engine.set_capabilities(PEER, true, false);
    f.receive(Side::Left, false);
    f.engine.on_peer_input(
        PEER,
        ControlMessage::PhysicalKey {
            usage: 227,
            pressed: true,
        },
    );
    let _ = recovery::commands(&f);
    f.engine.dispatch(SessionEvent::HotkeyPressed);
    assert!(
        recovery::commands(&f).contains(&PlatformCommand::InjectPhysicalKey {
            code: 29,
            pressed: false
        })
    );
    assert!(f.engine.injected.is_empty());
}
