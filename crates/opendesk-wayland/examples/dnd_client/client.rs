use std::sync::{Arc, Mutex};

use smithay_client_toolkit::compositor::CompositorState;
use smithay_client_toolkit::data_device_manager::DataDeviceManagerState;
use smithay_client_toolkit::data_device_manager::data_device::DataDevice;
use smithay_client_toolkit::data_device_manager::data_source::DragSource;
use smithay_client_toolkit::output::OutputState;
use smithay_client_toolkit::reexports::calloop::EventLoop;
use smithay_client_toolkit::reexports::calloop_wayland_source::WaylandSource;
use smithay_client_toolkit::registry::RegistryState;
use smithay_client_toolkit::seat::SeatState;
use smithay_client_toolkit::shell::WaylandSurface;
use smithay_client_toolkit::shell::wlr_layer::{
    Anchor, KeyboardInteractivity, Layer, LayerShell, LayerSurface,
};
use smithay_client_toolkit::shm::Shm;
use smithay_client_toolkit::shm::slot::{Buffer, SlotPool};
use wayland_client::globals::{GlobalList, registry_queue_init};
use wayland_client::protocol::wl_data_device_manager::DndAction;
use wayland_client::protocol::wl_pointer::WlPointer;
use wayland_client::protocol::wl_seat::WlSeat;
use wayland_client::protocol::wl_shm::Format;
use wayland_client::protocol::wl_surface::WlSurface;
use wayland_client::{Connection, QueueHandle};

use super::{DndResult, URI_LIST_MIME};

const NAMESPACE: &str = "opendesk-dnd";
const POOL_BYTES: usize = 1280 * 720 * 4;

#[derive(Clone, Default)]
pub struct DndRole {
    pub uri: Option<String>,
    pub accept_on_enter: bool,
    pub read_on_enter: bool,
    pub read_on_drop: bool,
    pub print_tokens: bool,
}

pub struct LayerEntry {
    pub layer: LayerSurface,
    pub buffer: Option<Buffer>,
    pub opaque: bool,
}

pub struct DndClient {
    pub registry_state: RegistryState,
    pub seat_state: SeatState,
    pub output_state: OutputState,
    pub shm: Shm,
    pub compositor: CompositorState,
    pub layer_shell: LayerShell,
    pub data_manager: DataDeviceManagerState,
    pub queue_handle: QueueHandle<DndClient>,
    pub role: DndRole,
    pub pool: SlotPool,
    pub seat: Option<WlSeat>,
    pub pointer: Option<WlPointer>,
    pub data_device: Option<DataDevice>,
    pub layers: Vec<LayerEntry>,
    pub icon: Option<WlSurface>,
    pub drag_source: Option<DragSource>,
    pub mapped: usize,
    pub pointer_entered: bool,
    pub last_button_serial: Option<u32>,
    pub data_enter: bool,
    pub data_leave: bool,
    pub data_drop: bool,
    pub send_count: usize,
    pub source_cancelled: bool,
    pub dnd_finished: bool,
    pub drag_started: bool,
    pub peeked_uri: Arc<Mutex<Option<String>>>,
    pub dropped_uri: Arc<Mutex<Option<String>>>,
}

impl DndClient {
    fn new(
        globals: &GlobalList,
        queue_handle: &QueueHandle<DndClient>,
        role: DndRole,
    ) -> DndResult<DndClient> {
        let shm = Shm::bind(globals, queue_handle)?;
        let pool = SlotPool::new(POOL_BYTES, &shm)?;
        Ok(DndClient {
            registry_state: RegistryState::new(globals),
            seat_state: SeatState::new(globals, queue_handle),
            output_state: OutputState::new(globals, queue_handle),
            shm,
            compositor: CompositorState::bind(globals, queue_handle)?,
            layer_shell: LayerShell::bind(globals, queue_handle)?,
            data_manager: DataDeviceManagerState::bind(globals, queue_handle)?,
            queue_handle: queue_handle.clone(),
            role,
            pool,
            seat: None,
            pointer: None,
            data_device: None,
            layers: Vec::new(),
            icon: None,
            drag_source: None,
            mapped: 0,
            pointer_entered: false,
            last_button_serial: None,
            data_enter: false,
            data_leave: false,
            data_drop: false,
            send_count: 0,
            source_cancelled: false,
            dnd_finished: false,
            drag_started: false,
            peeked_uri: Arc::new(Mutex::new(None)),
            dropped_uri: Arc::new(Mutex::new(None)),
        })
    }

    pub fn create_layer(&mut self, anchor: Anchor, width: u32, height: u32, opaque: bool) {
        let surface = self.compositor.create_surface(&self.queue_handle);
        let layer = self.layer_shell.create_layer_surface(
            &self.queue_handle,
            surface,
            Layer::Overlay,
            Some(NAMESPACE),
            None,
        );
        layer.set_anchor(anchor);
        layer.set_size(width, height);
        layer.set_exclusive_zone(-1);
        layer.set_keyboard_interactivity(KeyboardInteractivity::None);
        layer.commit();
        self.layers.push(LayerEntry {
            layer,
            buffer: None,
            opaque,
        });
    }

    pub fn create_icon(&mut self) -> DndResult<()> {
        let surface = self.compositor.create_surface(&self.queue_handle);
        let (buffer, canvas) = self.pool.create_buffer(1, 1, 4, Format::Argb8888)?;
        canvas.fill(0xFF);
        buffer.attach_to(&surface)?;
        surface.damage_buffer(0, 0, 1, 1);
        surface.commit();
        self.icon = Some(surface);
        Ok(())
    }

    pub fn paint_layer(&mut self, index: usize, width: u32, height: u32) -> DndResult<()> {
        let width = width.max(1) as i32;
        let height = height.max(1) as i32;
        let stride = width * 4;
        let opaque = match self.layers.get(index) {
            Some(entry) => entry.opaque,
            None => return Ok(()),
        };
        let (buffer, canvas) = self
            .pool
            .create_buffer(width, height, stride, Format::Argb8888)?;
        canvas.fill(if opaque { 0xFF } else { 0x00 });
        if let Some(entry) = self.layers.get_mut(index) {
            let surface = entry.layer.wl_surface();
            buffer.attach_to(surface)?;
            surface.damage_buffer(0, 0, width, height);
            entry.layer.commit();
            entry.buffer = Some(buffer);
        }
        Ok(())
    }

    pub fn begin_drag(&mut self, serial: u32) -> DndResult<()> {
        let data_device = self
            .data_device
            .as_ref()
            .ok_or("no data device on the seat")?;
        let origin = self
            .layers
            .first()
            .ok_or("no origin surface for the drag")?
            .layer
            .wl_surface()
            .clone();
        let source = self.data_manager.create_drag_and_drop_source(
            &self.queue_handle,
            [URI_LIST_MIME],
            DndAction::Copy,
        );
        source.start_drag(data_device, &origin, self.icon.as_ref(), serial);
        self.drag_started = true;
        if self.role.print_tokens {
            println!("drag started");
        }
        tracing::info!(serial, "start_drag issued");
        self.drag_source = Some(source);
        Ok(())
    }

    pub fn destroy_origin(&mut self) {
        if !self.layers.is_empty() {
            let entry = self.layers.remove(0);
            entry.layer.wl_surface().attach(None, 0, 0);
            entry.layer.wl_surface().commit();
            drop(entry);
        }
    }

    pub fn reset_pointer(&mut self) {
        self.pointer_entered = false;
        self.last_button_serial = None;
    }
}

pub fn connect(role: DndRole) -> DndResult<(EventLoop<'static, DndClient>, DndClient)> {
    let connection = Connection::connect_to_env()?;
    let (globals, mut event_queue) = registry_queue_init::<DndClient>(&connection)?;
    let queue_handle = event_queue.handle();
    let mut client = DndClient::new(&globals, &queue_handle, role)?;
    event_queue.roundtrip(&mut client)?;
    event_queue.roundtrip(&mut client)?;
    let event_loop = EventLoop::try_new()?;
    WaylandSource::new(connection, event_queue)
        .insert(event_loop.handle())
        .map_err(|error| error.error)?;
    Ok((event_loop, client))
}
