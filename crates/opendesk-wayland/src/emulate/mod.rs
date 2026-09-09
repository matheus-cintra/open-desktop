pub mod keyboard;
pub mod pointer;

use keyboard::VirtualKeyboard;
use pointer::VirtualPointer;

#[derive(Default)]
pub struct Emulator {
    pub pointer: Option<VirtualPointer>,
    pub keyboard: Option<VirtualKeyboard>,
}
