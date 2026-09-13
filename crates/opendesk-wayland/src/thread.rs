use std::sync::Arc;
use std::sync::atomic::AtomicU64;
use std::sync::mpsc;
use std::thread::JoinHandle;

use calloop::EventLoop;
use calloop::channel::{Channel, Event, Sender};
use calloop_wayland_source::WaylandSource;
use tokio::sync::mpsc::UnboundedSender;
use wayland_client::Connection;
use wayland_client::globals::registry_queue_init;

use crate::error::WaylandError;
use crate::events::{WaylandCommand, WaylandEvent};
use crate::outputs::output_geometries;
use crate::state::State;

const THREAD_NAME: &str = "opendesk-wayland";

pub struct WaylandHandle {
    pub commands: Sender<WaylandCommand>,
    pub drag_generation: Arc<AtomicU64>,
    join: JoinHandle<()>,
}

impl WaylandHandle {
    pub fn shutdown(self) -> Result<(), WaylandError> {
        if self.commands.send(WaylandCommand::Shutdown).is_err() {
            tracing::debug!("wayland thread already stopped");
        }
        self.join.join().map_err(|_| WaylandError::ThreadPanicked)
    }
}

pub fn spawn(events: UnboundedSender<WaylandEvent>) -> Result<WaylandHandle, WaylandError> {
    let (commands, channel) = calloop::channel::channel();
    let drag_generation = Arc::new(AtomicU64::new(0));
    let state_generation = drag_generation.clone();
    let (startup_sender, startup_receiver) = mpsc::channel();
    let join = std::thread::Builder::new()
        .name(THREAD_NAME.to_owned())
        .spawn(move || run_thread(events, channel, startup_sender, state_generation))?;
    startup_receiver
        .recv()
        .map_err(|_| WaylandError::StartupChannelClosed)??;
    Ok(WaylandHandle {
        commands,
        drag_generation,
        join,
    })
}

fn run_thread(
    events: UnboundedSender<WaylandEvent>,
    channel: Channel<WaylandCommand>,
    startup_sender: mpsc::Sender<Result<(), WaylandError>>,
    drag_generation: Arc<AtomicU64>,
) {
    let (mut event_loop, mut state) = match connect(events.clone(), channel, drag_generation) {
        Ok(ready) => ready,
        Err(error) => {
            tracing::error!(%error, "wayland thread failed to start");
            if startup_sender.send(Err(error)).is_err() {
                tracing::debug!("spawn caller went away before startup finished");
            }
            return;
        }
    };
    if startup_sender.send(Ok(())).is_err() {
        tracing::debug!("spawn caller went away before startup finished");
        return;
    }
    if let Err(error) = event_loop.run(None, &mut state, |_| {}) {
        tracing::error!(%error, "wayland event loop stopped");
        state.emit(WaylandEvent::Fatal {
            message: error.to_string(),
        });
    }
    tracing::info!("wayland thread finished");
}

fn connect(
    events: UnboundedSender<WaylandEvent>,
    channel: Channel<WaylandCommand>,
    drag_generation: Arc<AtomicU64>,
) -> Result<(EventLoop<'static, State>, State), WaylandError> {
    let connection = Connection::connect_to_env()?;
    let (globals, mut event_queue) = registry_queue_init::<State>(&connection)?;
    let queue_handle = event_queue.handle();
    let event_loop = EventLoop::<State>::try_new()?;
    let mut state = State::new(
        &globals,
        &queue_handle,
        events,
        event_loop.get_signal(),
        event_loop.handle(),
        drag_generation,
    )?;
    event_queue.roundtrip(&mut state)?;
    event_queue.roundtrip(&mut state)?;

    WaylandSource::new(connection, event_queue)
        .insert(event_loop.handle())
        .map_err(|error| error.error)?;
    event_loop
        .handle()
        .insert_source(channel, move |event, _, state: &mut State| match event {
            Event::Msg(command) => state.handle_command(&queue_handle, command),
            Event::Closed => {
                tracing::info!("command channel closed");
                state.loop_signal.stop();
            }
        })
        .map_err(|error| error.error)?;

    let outputs = output_geometries(&state.output_state);
    tracing::info!(?outputs, "wayland connection ready");
    state.mark_ready(WaylandEvent::Ready { outputs });
    Ok((event_loop, state))
}
