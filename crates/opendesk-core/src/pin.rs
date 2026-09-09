use std::time::{Duration, Instant};

use opendesk_proto::control::{PeerId, Token};

pub const PIN_DIGITS: usize = 6;
pub const MAX_ATTEMPTS: u8 = 3;
pub const PIN_TTL: Duration = Duration::from_secs(60);

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Pin(String);

impl Pin {
    pub fn generate() -> Pin {
        let value = rand::random_range(0..1_000_000u32);
        Pin(format!("{value:06}"))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn parse(text: &str) -> Option<Pin> {
        let trimmed = text.trim();
        let is_valid =
            trimmed.len() == PIN_DIGITS && trimmed.bytes().all(|byte| byte.is_ascii_digit());
        is_valid.then(|| Pin(trimmed.to_owned()))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PinVerdict {
    Accepted,
    Wrong { remaining_attempts: u8 },
    Locked,
    Expired,
}

#[derive(Debug)]
pub struct PairingAttempt {
    pin: Pin,
    created_at: Instant,
    failed_attempts: u8,
}

impl PairingAttempt {
    pub fn new(pin: Pin, now: Instant) -> PairingAttempt {
        PairingAttempt {
            pin,
            created_at: now,
            failed_attempts: 0,
        }
    }

    pub fn pin(&self) -> &Pin {
        &self.pin
    }

    pub fn is_expired(&self, now: Instant) -> bool {
        now.saturating_duration_since(self.created_at) >= PIN_TTL
    }

    pub fn verify(&mut self, candidate: &str, now: Instant) -> PinVerdict {
        if self.is_expired(now) {
            return PinVerdict::Expired;
        }
        if self.failed_attempts >= MAX_ATTEMPTS {
            return PinVerdict::Locked;
        }
        if Pin::parse(candidate).as_ref() == Some(&self.pin) {
            return PinVerdict::Accepted;
        }
        self.failed_attempts += 1;
        if self.failed_attempts >= MAX_ATTEMPTS {
            PinVerdict::Locked
        } else {
            PinVerdict::Wrong {
                remaining_attempts: MAX_ATTEMPTS - self.failed_attempts,
            }
        }
    }
}

pub fn generate_token() -> Token {
    let mut bytes = [0u8; 32];
    rand::fill(&mut bytes);
    Token(bytes)
}

pub fn generate_peer_id() -> PeerId {
    PeerId(uuid::Uuid::new_v4().into_bytes())
}
