use xkbcommon::xkb;

use crate::error::WaylandError;
use crate::events::HotkeySpec;

const EVDEV_TO_XKB_OFFSET: u32 = 8;

pub struct HotkeyMatcher {
    context: xkb::Context,
    state: Option<xkb::State>,
    spec: Option<HotkeySpec>,
    swallowed_code: Option<u32>,
}

impl HotkeyMatcher {
    pub fn new() -> HotkeyMatcher {
        HotkeyMatcher {
            context: xkb::Context::new(xkb::CONTEXT_NO_FLAGS),
            state: None,
            spec: None,
            swallowed_code: None,
        }
    }

    pub fn set_spec(&mut self, spec: HotkeySpec) {
        self.spec = Some(HotkeySpec {
            key: spec.key.to_lowercase(),
            ..spec
        });
    }

    pub fn set_keymap(&mut self, xkb_text: &str) -> Result<(), WaylandError> {
        let keymap = xkb::Keymap::new_from_string(
            &self.context,
            xkb_text.to_owned(),
            xkb::KEYMAP_FORMAT_TEXT_V1,
            xkb::KEYMAP_COMPILE_NO_FLAGS,
        )
        .ok_or(WaylandError::KeymapCompile)?;
        self.state = Some(xkb::State::new(&keymap));
        Ok(())
    }

    pub fn update_modifiers(&mut self, depressed: u32, latched: u32, locked: u32, group: u32) {
        if let Some(state) = self.state.as_mut() {
            state.update_mask(depressed, latched, locked, 0, 0, group);
        }
    }

    pub fn consume_press(&mut self, code: u32) -> bool {
        if self.matches(code) {
            self.swallowed_code = Some(code);
            return true;
        }
        false
    }

    pub fn consume_release(&mut self, code: u32) -> bool {
        if self.swallowed_code == Some(code) {
            self.swallowed_code = None;
            return true;
        }
        false
    }

    fn matches(&self, code: u32) -> bool {
        let (Some(spec), Some(state)) = (self.spec.as_ref(), self.state.as_ref()) else {
            return false;
        };
        let keysym = state.key_get_one_sym(xkb::Keycode::new(code + EVDEV_TO_XKB_OFFSET));
        if xkb::keysym_get_name(keysym).to_lowercase() != spec.key {
            return false;
        }
        let active = |name: &str| state.mod_name_is_active(name, xkb::STATE_MODS_EFFECTIVE);
        spec.ctrl == active(xkb::MOD_NAME_CTRL)
            && spec.alt == active(xkb::MOD_NAME_ALT)
            && spec.shift == active(xkb::MOD_NAME_SHIFT)
            && spec.logo == active(xkb::MOD_NAME_LOGO)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const KEY_ESC: u32 = 1;
    const KEY_F12: u32 = 88;

    fn us_keymap_text() -> String {
        let context = xkb::Context::new(xkb::CONTEXT_NO_FLAGS);
        let keymap = xkb::Keymap::new_from_names(
            &context,
            "",
            "",
            "us",
            "",
            None,
            xkb::KEYMAP_COMPILE_NO_FLAGS,
        )
        .unwrap();
        keymap.get_as_string(xkb::KEYMAP_FORMAT_TEXT_V1)
    }

    fn modifier_mask(matcher: &HotkeyMatcher, name: &str) -> u32 {
        let state = matcher.state.as_ref().unwrap();
        1 << state.get_keymap().mod_get_index(name)
    }

    fn matcher(spec: HotkeySpec) -> HotkeyMatcher {
        let mut matcher = HotkeyMatcher::new();
        matcher.set_keymap(&us_keymap_text()).unwrap();
        matcher.set_spec(spec);
        matcher
    }

    fn ctrl_alt_escape() -> HotkeySpec {
        HotkeySpec {
            ctrl: true,
            alt: true,
            shift: false,
            logo: false,
            key: "Escape".to_owned(),
        }
    }

    #[test]
    fn matches_only_with_the_exact_modifier_set() {
        let mut matcher = matcher(ctrl_alt_escape());
        assert!(!matcher.consume_press(KEY_ESC));

        let ctrl = modifier_mask(&matcher, xkb::MOD_NAME_CTRL);
        let alt = modifier_mask(&matcher, xkb::MOD_NAME_ALT);
        let shift = modifier_mask(&matcher, xkb::MOD_NAME_SHIFT);

        matcher.update_modifiers(ctrl, 0, 0, 0);
        assert!(!matcher.consume_press(KEY_ESC));

        matcher.update_modifiers(ctrl | alt | shift, 0, 0, 0);
        assert!(!matcher.consume_press(KEY_ESC));

        matcher.update_modifiers(ctrl | alt, 0, 0, 0);
        assert!(!matcher.consume_press(KEY_F12));
        assert!(matcher.consume_press(KEY_ESC));
    }

    #[test]
    fn release_of_the_swallowed_key_is_swallowed_once() {
        let mut matcher = matcher(HotkeySpec {
            ctrl: false,
            alt: false,
            shift: false,
            logo: false,
            key: "f12".to_owned(),
        });
        assert!(!matcher.consume_release(KEY_F12));
        assert!(matcher.consume_press(KEY_F12));
        assert!(!matcher.consume_release(KEY_ESC));
        assert!(matcher.consume_release(KEY_F12));
        assert!(!matcher.consume_release(KEY_F12));
    }

    #[test]
    fn nothing_matches_without_a_keymap() {
        let mut matcher = HotkeyMatcher::new();
        matcher.set_spec(ctrl_alt_escape());
        assert!(!matcher.consume_press(KEY_ESC));
    }
}
