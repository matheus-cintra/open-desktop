use std::collections::BTreeSet;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Release {
    Key(u32),
    Button(u32),
}

#[derive(Default, Debug, Clone)]
pub struct PressedInputs {
    keys: BTreeSet<u32>,
    buttons: BTreeSet<u32>,
}

impl PressedInputs {
    pub fn record_key(&mut self, code: u32, pressed: bool) {
        if pressed {
            self.keys.insert(code);
        } else {
            self.keys.remove(&code);
        }
    }

    pub fn record_button(&mut self, code: u32, pressed: bool) {
        if pressed {
            self.buttons.insert(code);
        } else {
            self.buttons.remove(&code);
        }
    }

    pub fn drain_releases(&mut self) -> Vec<Release> {
        let buttons = std::mem::take(&mut self.buttons);
        let keys = std::mem::take(&mut self.keys);
        buttons
            .into_iter()
            .map(Release::Button)
            .chain(keys.into_iter().map(Release::Key))
            .collect()
    }

    pub fn is_empty(&self) -> bool {
        self.keys.is_empty() && self.buttons.is_empty()
    }
}
