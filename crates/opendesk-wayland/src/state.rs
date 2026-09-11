use std::time::Instant;

use calloop::LoopSignal;
use smithay_client_toolkit::compositor::CompositorState;
use smithay_client_toolkit::output::OutputState;
use smithay_client_toolkit::registry::RegistryState;
use smithay_client_toolkit::seat::SeatState;
use smithay_client_toolkit::shell::wlr_layer::LayerShell;
use smithay_client_toolkit::shm::Shm;
use smithay_client_toolkit::shm::slot::SlotPool;
use tokio::sync::mpsc::UnboundedSender;
use wayland_client::QueueHandle;
use wayland_client::globals::GlobalList;

use crate::bar::Bars;
use crate::emulate::Emulator;
use crate::error::WaylandError;
use crate::events::WaylandEvent;
use crate::globals::Managers;
use crate::grab::Grab;
use crate::hotkey::HotkeyMatcher;
use crate::pointer::PointerTracking;
use crate::seat::Devices;
use crate::strip::Strips;

const INITIAL_POOL_BYTES: usize = 4096 * 4;

pub struct State {
    pub registry_state: RegistryState,
    pub output_state: OutputState,
    pub seat_state: SeatState,
    pub shm: Shm,
    pub pool: SlotPool,
    pub compositor: CompositorState,
    pub layer_shell: LayerShell,
    pub managers: Managers,
    pub devices: Devices,
    pub pointer: PointerTracking,
    pub strips: Strips,
    pub bars: Bars,
    pub grab: Grab,
    pub hotkey: HotkeyMatcher,
    pub emulator: Emulator,
    pub events: UnboundedSender<WaylandEvent>,
    pub started_at: Instant,
    pub ready: bool,
    pending_events: Vec<WaylandEvent>,
    pub loop_signal: LoopSignal,
}

impl State {
    pub fn new(
        globals: &GlobalList,
        queue_handle: &QueueHandle<State>,
        events: UnboundedSender<WaylandEvent>,
        loop_signal: LoopSignal,
    ) -> Result<State, WaylandError> {
        let shm = Shm::bind(globals, queue_handle)?;
        let pool = SlotPool::new(INITIAL_POOL_BYTES, &shm)?;
        Ok(State {
            registry_state: RegistryState::new(globals),
            output_state: OutputState::new(globals, queue_handle),
            seat_state: SeatState::new(globals, queue_handle),
            shm,
            pool,
            compositor: CompositorState::bind(globals, queue_handle)?,
            layer_shell: LayerShell::bind(globals, queue_handle)?,
            managers: Managers::bind(globals, queue_handle)?,
            devices: Devices::default(),
            pointer: PointerTracking::default(),
            strips: Strips::default(),
            bars: Bars::default(),
            grab: Grab::default(),
            hotkey: HotkeyMatcher::new(),
            emulator: Emulator::default(),
            events,
            started_at: Instant::now(),
            ready: false,
            pending_events: Vec::new(),
            loop_signal,
        })
    }

    pub fn emit(&mut self, event: WaylandEvent) {
        if !self.ready {
            self.pending_events.push(event);
            return;
        }
        self.send(event);
    }

    pub fn mark_ready(&mut self, ready: WaylandEvent) {
        self.ready = true;
        self.send(ready);
        for event in std::mem::take(&mut self.pending_events) {
            self.send(event);
        }
    }

    fn send(&self, event: WaylandEvent) {
        tracing::trace!(?event, "wayland event");
        if self.events.send(event).is_err() {
            tracing::warn!("event receiver dropped, stopping the wayland thread");
            self.loop_signal.stop();
        }
    }

    pub fn elapsed_millis(&self) -> u32 {
        u32::try_from(self.started_at.elapsed().as_millis()).unwrap_or(u32::MAX)
    }
}
