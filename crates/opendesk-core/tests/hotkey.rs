use opendesk_core::hotkey::{Hotkey, HotkeyError};

fn hotkey(ctrl: bool, alt: bool, shift: bool, logo: bool, key: &str) -> Hotkey {
    Hotkey {
        ctrl,
        alt,
        shift,
        logo,
        key: key.to_owned(),
    }
}

#[test]
fn parses_modifier_aliases_and_lowercases_the_key() {
    assert_eq!(
        "ctrl+alt+escape".parse(),
        Ok(hotkey(true, true, false, false, "escape"))
    );
    assert_eq!(
        "Control+Shift+F12".parse(),
        Ok(hotkey(true, false, true, false, "f12"))
    );
    assert_eq!(
        "super+q".parse(),
        Ok(hotkey(false, false, false, true, "q"))
    );
    assert_eq!(
        "meta + q".parse(),
        Ok(hotkey(false, false, false, true, "q"))
    );
    assert_eq!(
        "win+logo+q".parse(),
        Ok(hotkey(false, false, false, true, "q"))
    );
    assert_eq!(
        "escape".parse(),
        Ok(hotkey(false, false, false, false, "escape"))
    );
}

#[test]
fn rejects_malformed_hotkeys() {
    assert_eq!(
        "ctrl+alt".parse::<Hotkey>(),
        Err(HotkeyError::MissingKey("ctrl+alt".to_owned()))
    );
    assert_eq!(
        "ctrl+a+b".parse::<Hotkey>(),
        Err(HotkeyError::MultipleKeys(
            "ctrl+a+b".to_owned(),
            "a".to_owned(),
            "b".to_owned()
        ))
    );
    assert_eq!(
        "ctrl++a".parse::<Hotkey>(),
        Err(HotkeyError::EmptyToken("ctrl++a".to_owned()))
    );
    assert_eq!(
        "".parse::<Hotkey>(),
        Err(HotkeyError::EmptyToken(String::new()))
    );
}

#[test]
fn display_is_canonical() {
    let parsed: Hotkey = "Shift+Win+Control+Alt+Q".parse().unwrap();
    assert_eq!(parsed.to_string(), "ctrl+alt+shift+super+q");
    assert_eq!(parsed.to_string().parse::<Hotkey>(), Ok(parsed));
}

#[test]
fn matches_compares_modifiers_and_key_case_insensitively() {
    let parsed: Hotkey = "ctrl+alt+escape".parse().unwrap();
    assert!(parsed.matches(true, true, false, false, "Escape"));
    assert!(!parsed.matches(true, false, false, false, "escape"));
    assert!(!parsed.matches(true, true, true, false, "escape"));
    assert!(!parsed.matches(true, true, false, false, "q"));
}

#[test]
fn serializes_as_a_string() {
    let parsed: Hotkey = "ctrl+alt+escape".parse().unwrap();
    let toml_text = toml::to_string(&serde_wrapper::Wrapper {
        hotkey: parsed.clone(),
    })
    .unwrap();
    assert_eq!(toml_text, "hotkey = \"ctrl+alt+escape\"\n");
    let back: serde_wrapper::Wrapper = toml::from_str(&toml_text).unwrap();
    assert_eq!(back.hotkey, parsed);
    assert!(toml::from_str::<serde_wrapper::Wrapper>("hotkey = \"ctrl+alt\"").is_err());
}

mod serde_wrapper {
    use opendesk_core::hotkey::Hotkey;
    use serde::{Deserialize, Serialize};

    #[derive(Serialize, Deserialize)]
    pub struct Wrapper {
        pub hotkey: Hotkey,
    }
}
