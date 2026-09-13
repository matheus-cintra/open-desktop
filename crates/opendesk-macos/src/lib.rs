#![cfg(target_os = "macos")]
use opendesk_platform::{PlatformCommand, PlatformEvent};
use std::ffi::{CStr, CString, c_char};
use std::sync::{Arc, Mutex, OnceLock, atomic::AtomicU64};
use tokio::sync::mpsc::UnboundedSender;

static EVENTS: Mutex<Option<UnboundedSender<PlatformEvent>>> = Mutex::new(None);
type RequestHandler = Box<dyn Fn(&str) -> String + Send + Sync>;
static REQUEST: OnceLock<RequestHandler> = OnceLock::new();
unsafe extern "C" {
    fn od_run(
        event: extern "C" fn(*const c_char),
        request: extern "C" fn(*const c_char) -> *mut c_char,
        free: extern "C" fn(*mut c_char),
        clipboard: extern "C" fn(*const c_char, *const u8, usize),
    );
    fn od_command(json: *const c_char);
    fn od_health() -> i32;
    fn od_quit();
    fn od_permission_probe() -> i32;
    fn od_permission_relaunch(parent: i32) -> i32;
    fn od_set_clipboard(mime: *const c_char, bytes: *const u8, length: usize);
}
extern "C" fn event(json: *const c_char) {
    if json.is_null() {
        return;
    }
    // Native strings are valid for the duration of this synchronous callback.
    let text = unsafe { CStr::from_ptr(json) }.to_string_lossy();
    match serde_json::from_str::<PlatformEvent>(&text) {
        Ok(event) => {
            if let Ok(sender) = EVENTS.lock()
                && let Some(sender) = sender.as_ref()
            {
                let _ = sender.send(event);
            }
        }
        Err(error) => tracing::warn!(%error, "invalid native event"),
    }
}
extern "C" fn clipboard(mime: *const c_char, bytes: *const u8, length: usize) {
    if mime.is_null() || bytes.is_null() || length > 10 * 1024 * 1024 {
        return;
    }
    // Native NSData and MIME stay alive until this callback returns; copy before enqueueing.
    let content = opendesk_platform::ClipboardContent {
        mime: unsafe { CStr::from_ptr(mime) }
            .to_string_lossy()
            .into_owned(),
        bytes: unsafe { std::slice::from_raw_parts(bytes, length) }.to_vec(),
    };
    if let Ok(sender) = EVENTS.lock()
        && let Some(sender) = sender.as_ref()
    {
        let _ = sender.send(PlatformEvent::ClipboardChanged { content });
    }
}
extern "C" fn request(json: *const c_char) -> *mut c_char {
    if json.is_null() {
        return std::ptr::null_mut();
    }
    let text = unsafe { CStr::from_ptr(json) }.to_string_lossy();
    let reply = REQUEST
        .get()
        .map(|handler| handler(&text))
        .unwrap_or_else(|| "null".into());
    CString::new(reply)
        .map(CString::into_raw)
        .unwrap_or(std::ptr::null_mut())
}
extern "C" fn free_reply(ptr: *mut c_char) {
    if !ptr.is_null() {
        drop(unsafe { CString::from_raw(ptr) });
    }
}
pub fn run_app(handler: impl Fn(&str) -> String + Send + Sync + 'static) {
    let _ = REQUEST.set(Box::new(handler));
    unsafe {
        od_run(event, request, free_reply, clipboard);
    }
}
pub fn health() -> i32 {
    unsafe { od_health() }
}
#[derive(Clone)]
pub struct Commands;
impl Commands {
    pub fn send(&self, command: PlatformCommand) -> anyhow::Result<()> {
        if let PlatformCommand::SetClipboard { content } = command {
            let mime = CString::new(content.mime)?;
            unsafe {
                od_set_clipboard(mime.as_ptr(), content.bytes.as_ptr(), content.bytes.len());
            }
            return Ok(());
        }
        let text = CString::new(serde_json::to_string(&command)?)?;
        unsafe {
            od_command(text.as_ptr());
        }
        Ok(())
    }
}
pub struct PlatformHandle {
    pub commands: Commands,
    pub drag_generation: Arc<AtomicU64>,
}
impl PlatformHandle {
    pub fn shutdown(self) -> anyhow::Result<()> {
        self.commands.send(PlatformCommand::Shutdown)?;
        if let Ok(mut sender) = EVENTS.lock() {
            *sender = None;
        }
        Ok(())
    }
}
pub fn spawn(events: UnboundedSender<PlatformEvent>) -> anyhow::Result<PlatformHandle> {
    let mut sender = EVENTS
        .lock()
        .map_err(|_| anyhow::anyhow!("native event mutex poisoned"))?;
    anyhow::ensure!(sender.is_none(), "macOS backend already running");
    *sender = Some(events);
    let started = CString::new("\"BackendStarted\"")?;
    unsafe {
        od_command(started.as_ptr());
    }
    // Native startup retries until the AppKit loop and permissions are ready.
    Ok(PlatformHandle {
        commands: Commands,
        drag_generation: Arc::new(AtomicU64::new(0)),
    })
}

/// Request main-thread input cleanup and application termination after engine shutdown.
pub fn quit() {
    unsafe {
        od_quit();
    }
}

/// Fresh-process permission check, without prompts, input capture or daemon startup.
pub fn permission_probe() -> u8 {
    unsafe { od_permission_probe() as u8 }
}
pub fn permission_relaunch(parent: i32) -> u8 {
    unsafe { od_permission_relaunch(parent) as u8 }
}
