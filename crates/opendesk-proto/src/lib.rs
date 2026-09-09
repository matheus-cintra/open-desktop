pub mod codec;
pub mod control;
pub mod input;
pub mod transfer;

pub const PROTOCOL_VERSION: u16 = 1;
pub const DEFAULT_PORT: u16 = 47820;
pub const SERVICE_TYPE: &str = "_opendesk._tcp.local.";
