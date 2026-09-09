use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rgba {
    pub red: u8,
    pub green: u8,
    pub blue: u8,
    pub alpha: u8,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum ColorError {
    #[error("color `{0}` must start with `#`")]
    MissingHash(String),
    #[error("color `{0}` must have 6 or 8 hexadecimal digits")]
    BadLength(String),
    #[error("color `{0}` contains a non-hexadecimal digit")]
    BadDigit(String),
}

impl FromStr for Rgba {
    type Err = ColorError;

    fn from_str(text: &str) -> Result<Rgba, ColorError> {
        let trimmed = text.trim();
        let digits = trimmed
            .strip_prefix('#')
            .ok_or_else(|| ColorError::MissingHash(trimmed.to_owned()))?;
        if digits.len() != 6 && digits.len() != 8 {
            return Err(ColorError::BadLength(trimmed.to_owned()));
        }
        let channel = |index: usize| -> Result<u8, ColorError> {
            digits
                .get(index * 2..index * 2 + 2)
                .and_then(|pair| u8::from_str_radix(pair, 16).ok())
                .ok_or_else(|| ColorError::BadDigit(trimmed.to_owned()))
        };
        let alpha = if digits.len() == 8 { channel(3)? } else { 0xFF };
        Ok(Rgba {
            red: channel(0)?,
            green: channel(1)?,
            blue: channel(2)?,
            alpha,
        })
    }
}

impl fmt::Display for Rgba {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "#{:02x}{:02x}{:02x}{:02x}",
            self.red, self.green, self.blue, self.alpha
        )
    }
}

impl Serialize for Rgba {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_string())
    }
}

impl<'de> Deserialize<'de> for Rgba {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Rgba, D::Error> {
        let text = String::deserialize(deserializer)?;
        text.parse().map_err(serde::de::Error::custom)
    }
}
