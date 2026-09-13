use super::*;

impl Engine {
    pub async fn run(mut self, mut channels: EngineChannels) -> anyhow::Result<()> {
        self.apply_hotkey();
        self.apply_bar_style();
        let (monitor_requests, mut monitor_events) =
            super::monitor::spawn(super::monitor::socket_path());
        let mut monitor_tick = tokio::time::interval(Duration::from_millis(25));
        let mut ticker = tokio::time::interval(TICK);
        let outcome = loop {
            tokio::select! {
                Some(event) = channels.wayland_events.recv() => self.on_wayland(event),
                Some(event) = channels.tcp_events.recv() => self.on_tcp(event),
                Some(event) = channels.udp_events.recv() => self.on_udp(event),
                Some(event) = channels.discovery_events.recv() => self.on_discovery(event),
                Some((request, reply)) = channels.ipc_requests.recv() => self.on_ipc(request, reply),
                Some(ConfigChanged) = channels.config_events.recv() => {
                    self.config_dirty_since = Some(Instant::now());
                }
                Some(event) = channels.compositor_events.recv() => match event {
                    CompositorEvent::LeftReleased(at) => self.on_left_released(at),
                    CompositorEvent::EmergencyRelease => self.dispatch(SessionEvent::HotkeyPressed),
                },
                Some(event) = monitor_events.recv() => self.on_monitor(event),
                _ = monitor_tick.tick() => self.poll_compositor(Instant::now(), &monitor_requests),
                _ = ticker.tick() => self.on_tick(Instant::now()),
                _ = &mut channels.shutdown => break Ok(()),
            }
            if let Some(message) = self.fatal.take() {
                break Err(anyhow::anyhow!(message));
            }
        };
        if let Err(error) = self.sinks.wayland.shutdown() {
            warn!(%error, "wayland thread did not shut down cleanly");
        }
        outcome
    }
}
