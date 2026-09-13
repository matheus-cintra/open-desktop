#[cfg(target_os = "macos")]
pub use opendesk_macos::{PlatformHandle, spawn};
pub use opendesk_platform::*;
#[cfg(target_os = "linux")]
pub use opendesk_wayland::{WaylandHandle as PlatformHandle, spawn};
