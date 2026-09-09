use opendesk_core::pressed::{PressedInputs, Release};

#[test]
fn drains_buttons_before_keys_and_forgets_released_ones() {
    let mut pressed = PressedInputs::default();
    assert!(pressed.is_empty());
    pressed.record_key(30, true);
    pressed.record_key(16, true);
    pressed.record_key(30, false);
    pressed.record_button(272, true);
    pressed.record_button(273, true);
    pressed.record_button(273, false);
    assert!(!pressed.is_empty());
    assert_eq!(
        pressed.drain_releases(),
        vec![Release::Button(272), Release::Key(16)]
    );
    assert!(pressed.is_empty());
    assert!(pressed.drain_releases().is_empty());
}
