use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Hotkey {
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
    pub logo: bool,
    pub key: String,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum HotkeyError {
    #[error("hotkey `{0}` has an empty token")]
    EmptyToken(String),
    #[error("hotkey `{0}` has no key besides modifiers")]
    MissingKey(String),
    #[error("hotkey `{0}` has more than one key: `{1}` and `{2}`")]
    MultipleKeys(String, String, String),
}

enum Modifier {
    Ctrl,
    Alt,
    Shift,
    Logo,
}

fn modifier_from_word(word: &str) -> Option<Modifier> {
    match word {
        "ctrl" | "control" => Some(Modifier::Ctrl),
        "alt" => Some(Modifier::Alt),
        "shift" => Some(Modifier::Shift),
        "super" | "logo" | "meta" | "win" => Some(Modifier::Logo),
        _ => None,
    }
}

impl FromStr for Hotkey {
    type Err = HotkeyError;

    fn from_str(text: &str) -> Result<Hotkey, HotkeyError> {
        let mut hotkey = Hotkey {
            ctrl: false,
            alt: false,
            shift: false,
            logo: false,
            key: String::new(),
        };
        for token in text.split('+') {
            let word = token.trim().to_ascii_lowercase();
            if word.is_empty() {
                return Err(HotkeyError::EmptyToken(text.to_owned()));
            }
            match modifier_from_word(&word) {
                Some(Modifier::Ctrl) => hotkey.ctrl = true,
                Some(Modifier::Alt) => hotkey.alt = true,
                Some(Modifier::Shift) => hotkey.shift = true,
                Some(Modifier::Logo) => hotkey.logo = true,
                None if hotkey.key.is_empty() => hotkey.key = word,
                None => {
                    return Err(HotkeyError::MultipleKeys(text.to_owned(), hotkey.key, word));
                }
            }
        }
        if hotkey.key.is_empty() {
            return Err(HotkeyError::MissingKey(text.to_owned()));
        }
        Ok(hotkey)
    }
}

impl Hotkey {
    pub fn matches(&self, ctrl: bool, alt: bool, shift: bool, logo: bool, key: &str) -> bool {
        self.ctrl == ctrl
            && self.alt == alt
            && self.shift == shift
            && self.logo == logo
            && self.key.eq_ignore_ascii_case(key)
    }
}

impl fmt::Display for Hotkey {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let modifiers = [
            (self.ctrl, "ctrl"),
            (self.alt, "alt"),
            (self.shift, "shift"),
            (self.logo, "super"),
        ];
        for (enabled, word) in modifiers {
            if enabled {
                formatter.write_str(word)?;
                formatter.write_str("+")?;
            }
        }
        formatter.write_str(&self.key)
    }
}

impl Serialize for Hotkey {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_string())
    }
}

impl<'de> Deserialize<'de> for Hotkey {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Hotkey, D::Error> {
        let text = String::deserialize(deserializer)?;
        text.parse().map_err(serde::de::Error::custom)
    }
}
