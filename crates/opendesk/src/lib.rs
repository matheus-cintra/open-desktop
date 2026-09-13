pub mod cli;
pub mod daemon;
pub mod i18n;
#[cfg(target_os = "macos")]
pub mod macos_app;
pub mod platform;

rust_i18n::i18n!("../../locales", fallback = "en");

pub mod gui;
