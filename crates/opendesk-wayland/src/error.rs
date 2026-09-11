use std::io;

use opendesk_proto::control::Side;
use smithay_client_toolkit::error::GlobalError;
use smithay_client_toolkit::shm::CreatePoolError;
use smithay_client_toolkit::shm::slot::{ActivateSlotError, CreateBufferError};
use wayland_client::globals::{BindError, GlobalError as RegistryGlobalError};
use wayland_client::{ConnectError, DispatchError};

#[derive(Debug, thiserror::Error)]
pub enum WaylandError {
    #[error("failed to connect to the wayland display: {0}")]
    Connect(#[from] ConnectError),
    #[error("failed to read the wayland registry: {0}")]
    Registry(#[from] RegistryGlobalError),
    #[error("required global `{interface}` is missing: {source}")]
    MissingGlobal {
        interface: &'static str,
        source: BindError,
    },
    #[error("failed to bind a wayland global: {0}")]
    Bind(#[from] BindError),
    #[error("wayland dispatch failed: {0}")]
    Dispatch(#[from] DispatchError),
    #[error("event loop error: {0}")]
    EventLoop(#[from] calloop::Error),
    #[error("shared memory pool error: {0}")]
    ShmPool(#[from] CreatePoolError),
    #[error("shared memory buffer error: {0}")]
    ShmBuffer(#[from] CreateBufferError),
    #[error("buffer could not be attached: {0}")]
    BufferAttach(#[from] ActivateSlotError),
    #[error("wayland global error: {0}")]
    Global(#[from] GlobalError),
    #[error("failed to paint the bar: {0}")]
    Paint(&'static str),
    #[error("no output has an edge on side `{0}` near {1}")]
    NoOutputOnEdge(Side, f64),
    #[error("io error: {0}")]
    Io(#[from] io::Error),
    #[error("keymap is not valid utf-8")]
    KeymapNotUtf8,
    #[error("keymap could not be compiled by xkbcommon")]
    KeymapCompile,
    #[error("wayland thread exited before reporting its startup result")]
    StartupChannelClosed,
    #[error("wayland thread is no longer running")]
    ThreadGone,
    #[error("wayland thread panicked")]
    ThreadPanicked,
    #[error("no seat with a pointer is available")]
    NoPointer,
    #[error("no seat with a keyboard is available")]
    NoKeyboard,
    #[error("no strip currently has pointer focus")]
    NoFocusedStrip,
    #[error("output `{0}` is not connected")]
    UnknownOutput(String),
}
