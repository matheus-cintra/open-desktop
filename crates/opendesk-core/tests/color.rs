use opendesk_core::color::{ColorError, Rgba};

#[test]
fn parses_rgb_and_rgba() {
    assert_eq!(
        "#5e81acCC".parse(),
        Ok(Rgba {
            red: 0x5e,
            green: 0x81,
            blue: 0xac,
            alpha: 0xcc
        })
    );
    assert_eq!(
        "#FF0000".parse(),
        Ok(Rgba {
            red: 0xff,
            green: 0,
            blue: 0,
            alpha: 0xff
        })
    );
}

#[test]
fn rejects_malformed_colors() {
    assert_eq!(
        "5e81ac".parse::<Rgba>(),
        Err(ColorError::MissingHash("5e81ac".to_owned()))
    );
    assert_eq!(
        "#5e81a".parse::<Rgba>(),
        Err(ColorError::BadLength("#5e81a".to_owned()))
    );
    assert_eq!(
        "#5e81zz".parse::<Rgba>(),
        Err(ColorError::BadDigit("#5e81zz".to_owned()))
    );
}

#[test]
fn display_is_lowercase_with_alpha() {
    let color: Rgba = "#5E81AC".parse().unwrap();
    assert_eq!(color.to_string(), "#5e81acff");
    assert_eq!(color.to_string().parse::<Rgba>(), Ok(color));
}
