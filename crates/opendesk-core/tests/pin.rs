use std::time::{Duration, Instant};

use opendesk_core::pin::{
    MAX_ATTEMPTS, PIN_TTL, PairingAttempt, Pin, PinVerdict, generate_peer_id, generate_token,
};

#[test]
fn generated_pin_has_six_digits() {
    for _ in 0..50 {
        let pin = Pin::generate();
        assert_eq!(pin.as_str().len(), 6);
        assert!(pin.as_str().bytes().all(|byte| byte.is_ascii_digit()));
        assert_eq!(Pin::parse(pin.as_str()), Some(pin));
    }
}

#[test]
fn parse_rejects_malformed_pins() {
    assert_eq!(Pin::parse("12345"), None);
    assert_eq!(Pin::parse("1234567"), None);
    assert_eq!(Pin::parse("12345a"), None);
    assert!(Pin::parse(" 123456 ").is_some());
}

#[test]
fn verify_accepts_the_right_pin() {
    let now = Instant::now();
    let pin = Pin::parse("123456").unwrap();
    let mut attempt = PairingAttempt::new(pin.clone(), now);
    assert_eq!(attempt.pin(), &pin);
    assert_eq!(attempt.verify("123456", now), PinVerdict::Accepted);
    assert_eq!(attempt.verify("123456", now), PinVerdict::Accepted);
}

#[test]
fn verify_counts_down_then_locks() {
    let now = Instant::now();
    let mut attempt = PairingAttempt::new(Pin::parse("123456").unwrap(), now);
    assert_eq!(
        attempt.verify("000000", now),
        PinVerdict::Wrong {
            remaining_attempts: MAX_ATTEMPTS - 1
        }
    );
    assert_eq!(
        attempt.verify("000000", now),
        PinVerdict::Wrong {
            remaining_attempts: MAX_ATTEMPTS - 2
        }
    );
    assert_eq!(attempt.verify("000000", now), PinVerdict::Locked);
    assert_eq!(attempt.verify("123456", now), PinVerdict::Locked);
}

#[test]
fn verify_expires_after_ttl() {
    let now = Instant::now();
    let mut attempt = PairingAttempt::new(Pin::parse("123456").unwrap(), now);
    assert!(!attempt.is_expired(now + PIN_TTL - Duration::from_millis(1)));
    assert!(attempt.is_expired(now + PIN_TTL));
    assert_eq!(attempt.verify("123456", now + PIN_TTL), PinVerdict::Expired);
}

#[test]
fn tokens_and_peer_ids_are_random() {
    assert_ne!(generate_token().0, generate_token().0);
    assert_ne!(generate_peer_id(), generate_peer_id());
    assert_ne!(generate_token().0, [0u8; 32]);
}
