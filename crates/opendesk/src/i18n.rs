const PORTUGUESE: &str = "pt-BR";
const ENGLISH: &str = "en";

pub fn detect_locale(candidates: &[Option<String>]) -> &'static str {
    let selected = candidates
        .iter()
        .flatten()
        .map(|value| value.trim())
        .find(|value| !value.is_empty() && *value != "C" && *value != "POSIX");
    match selected {
        Some(value) if value.to_ascii_lowercase().starts_with("pt") => PORTUGUESE,
        _ => ENGLISH,
    }
}

pub fn init_locale_from_env() -> &'static str {
    let candidates = [
        std::env::var("LC_ALL").ok(),
        std::env::var("LC_MESSAGES").ok(),
        std::env::var("LANG").ok(),
    ];
    let locale = detect_locale(&candidates);
    rust_i18n::set_locale(locale);
    locale
}

#[cfg(test)]
mod tests {
    use super::*;

    fn owned(value: &str) -> Option<String> {
        Some(value.to_owned())
    }

    #[test]
    fn portuguese_prefix_selects_pt_br() {
        assert_eq!(detect_locale(&[owned("pt_BR.UTF-8")]), "pt-BR");
        assert_eq!(detect_locale(&[None, owned("pt_PT")]), "pt-BR");
        assert_eq!(detect_locale(&[None, None, owned("PT_BR")]), "pt-BR");
    }

    #[test]
    fn anything_else_falls_back_to_english() {
        assert_eq!(detect_locale(&[owned("en_US.UTF-8")]), "en");
        assert_eq!(detect_locale(&[owned("es_ES")]), "en");
        assert_eq!(detect_locale(&[None, None, None]), "en");
        assert_eq!(detect_locale(&[owned("C"), owned("pt_BR")]), "pt-BR");
        assert_eq!(detect_locale(&[owned(""), owned("pt_BR")]), "pt-BR");
    }

    #[test]
    fn earlier_variables_take_precedence() {
        assert_eq!(detect_locale(&[owned("en_US"), owned("pt_BR")]), "en");
        assert_eq!(detect_locale(&[owned("pt_BR"), owned("en_US")]), "pt-BR");
    }
}
