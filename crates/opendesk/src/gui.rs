//! A separate native process; all configuration and trust stay in the daemon.
use crate::daemon::ipc::{DiscoveredPeerReport, IpcRequest, IpcResponse, MapReport};
use eframe::egui::{self, Color32, Rect, Sense, Stroke, Vec2};
use opendesk_proto::map::{DesktopMap, Revision, Screen};
use std::{
    sync::mpsc,
    time::{Duration, Instant},
};

pub fn run() -> anyhow::Result<()> {
    let path = crate::daemon::ipc::socket_path()?.with_file_name("gui.lock");
    std::fs::create_dir_all(
        path.parent()
            .ok_or_else(|| anyhow::anyhow!("Diretório indisponível"))?,
    )?;
    let mut lock = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(path)?;
    if lock.try_lock().is_err() {
        use std::io::Write;
        lock.write_all(b"focus")?;
        return Ok(());
    }
    let (requests, rx) = mpsc::channel();
    let (tx, responses) = mpsc::channel();
    std::thread::spawn(move || {
        let runtime = match tokio::runtime::Runtime::new() {
            Ok(r) => r,
            Err(e) => {
                let _ = tx.send((
                    false,
                    IpcResponse::Error {
                        message: e.to_string(),
                    },
                ));
                return;
            }
        };
        for request in rx {
            let applying = matches!(request, IpcRequest::MapApply { .. });
            let response = runtime
                .block_on(async {
                    let path = crate::daemon::ipc::socket_path()?;
                    tokio::time::timeout(
                        Duration::from_secs(15),
                        crate::daemon::ipc::client::request(&path, request),
                    )
                    .await?
                })
                .unwrap_or_else(|e| IpcResponse::Error {
                    message: e.to_string(),
                });
            if tx.send((applying, response)).is_err() {
                break;
            }
        }
    });
    requests.send(IpcRequest::MapGet)?;
    let app = MapApp {
        focus: lock.try_clone()?,
        last_focus: lock.metadata()?.modified().ok(),
        requests,
        responses,
        report: None,
        draft: None,
        base: None,
        dirty: false,
        message: String::new(),
        zoom: 0.14,
        pan: Vec2::ZERO,
        discover: Vec::new(),
        adding: false,
        pin: String::new(),
        pin_required: false,
        last_poll: Instant::now(),
        busy: false,
        drag_start: None,
    };
    eframe::run_native(
        "Open Desktop — Organizar computadores",
        eframe::NativeOptions {
            renderer: eframe::Renderer::Wgpu,
            viewport: egui::ViewportBuilder::default()
                .with_inner_size([1050.0, 680.0])
                .with_min_inner_size([700.0, 480.0]),
            ..Default::default()
        },
        Box::new(|_| Ok(Box::new(app))),
    )
    .map_err(|e| anyhow::anyhow!(e.to_string()))?;
    drop(lock);
    Ok(())
}
struct MapApp {
    focus: std::fs::File,
    last_focus: Option<std::time::SystemTime>,
    requests: mpsc::Sender<IpcRequest>,
    responses: mpsc::Receiver<(bool, IpcResponse)>,
    report: Option<MapReport>,
    draft: Option<DesktopMap>,
    base: Option<Revision>,
    dirty: bool,
    message: String,
    zoom: f32,
    pan: Vec2,
    discover: Vec<DiscoveredPeerReport>,
    adding: bool,
    pin: String,
    pin_required: bool,
    last_poll: Instant,
    busy: bool,
    drag_start: Option<(usize, i32, i32)>,
}
impl MapApp {
    fn send(&mut self, request: IpcRequest) {
        if self.requests.send(request).is_err() {
            self.message = "Conexão local encerrada".into();
        }
    }
    fn reset(&mut self) {
        if let Some(report) = &self.report {
            self.draft = Some(report.map.clone());
            self.base = report.applied.then_some(report.map.revision);
            self.dirty = false;
        }
    }
    fn receive(&mut self) {
        while let Ok((applying, response)) = self.responses.try_recv() {
            match response {
                IpcResponse::Map(report) => {
                    let changed = self.base != report.applied.then_some(report.map.revision);
                    if applying {
                        self.dirty = false;
                        self.busy = false;
                        self.message = "Mapa aplicado".into();
                    }
                    if self.dirty && changed {
                        self.message="O mapa mudou em outro computador. Seu rascunho foi mantido; cancele para revisar o mapa atual.".into();
                    }
                    self.report = Some(report);
                    if !self.dirty {
                        self.reset();
                    }
                }
                IpcResponse::MapConflict(report) => {
                    self.busy = false;
                    self.report = Some(report);
                    self.message="Conflito de edição. Seu rascunho foi preservado pelo daemon. Cancele para revisar a nova disposição.".into();
                }
                IpcResponse::Discovered(peers) => self.discover = peers,
                IpcResponse::PinRequired => {
                    self.pin_required = true;
                    self.message = "Digite o PIN exibido no outro computador".into();
                }
                IpcResponse::Error { message } => {
                    self.busy = false;
                    self.message = message;
                }
                IpcResponse::Ok => {
                    self.message = "Concluído".into();
                    self.send(IpcRequest::MapGet);
                }
                _ => {}
            }
        }
    }
    fn toolbar(&mut self, ui: &mut egui::Ui) {
        ui.heading("Seus computadores");
        ui.label("Arraste para organizar. Encaixe as bordas para criar passagens.");
        ui.add_space(12.0);
        ui.horizontal(|ui| {
            let valid = self
                .draft
                .as_ref()
                .is_some_and(|m| opendesk_core::desktop_map::validate(m).is_ok());
            if ui
                .add_enabled(valid && !self.busy, egui::Button::new("Aplicar"))
                .clicked()
                && let Some(map) = self.draft.clone()
            {
                self.busy = true;
                self.send(IpcRequest::MapApply {
                    base: self.base,
                    map,
                });
            }
            if ui.button("Cancelar").clicked() {
                self.reset();
                self.message.clear();
            }
            if ui.button("Identificar telas").clicked() {
                self.send(IpcRequest::Identify);
            }
            if ui.button("Adicionar computador").clicked() {
                self.adding = true;
                self.send(IpcRequest::Discover);
            }
            ui.separator();
            ui.add(egui::Slider::new(&mut self.zoom, 0.03..=0.4).text("Zoom"));
            if ui.button("Centralizar").clicked() {
                self.pan = Vec2::ZERO;
                ui.ctx()
                    .data_mut(|d| d.remove::<Vec2>(egui::Id::new("map-center")));
            }
        });
        if let Some(error) = self
            .draft
            .as_ref()
            .and_then(|m| opendesk_core::desktop_map::validate(m).err())
        {
            ui.colored_label(Color32::LIGHT_RED, error);
        }
        if !self.message.is_empty() {
            ui.label(&self.message);
        }
        if let Some(error) = self.report.as_ref().and_then(|r| r.error.as_ref()) {
            ui.colored_label(Color32::YELLOW, error);
        }
    }
    fn canvas(&mut self, ui: &mut egui::Ui) {
        let (canvas, response) = ui.allocate_exact_size(ui.available_size(), Sense::drag());
        if response.dragged() {
            self.pan += ui.input(|i| i.pointer.delta());
        }
        let Some(map) = self.draft.as_mut() else {
            ui.label("Conectando ao serviço Open Desktop…");
            return;
        };
        let min_x = map.screens.iter().map(|s| s.x).min().unwrap_or(0) as f32;
        let max_x = map.screens.iter().map(|s| s.x + s.width).max().unwrap_or(0) as f32;
        let min_y = map.screens.iter().map(|s| s.y).min().unwrap_or(0) as f32;
        let max_y = map
            .screens
            .iter()
            .map(|s| s.y + s.height)
            .max()
            .unwrap_or(0) as f32;
        // Stable origin while dragging: don't recenter around each new position.
        let center = ui.ctx().data_mut(|d| {
            *d.get_temp_mut_or_insert_with(egui::Id::new("map-center"), || {
                Vec2::new((min_x + max_x) / 2.0, (min_y + max_y) / 2.0)
            })
        });
        let origin = canvas.center() + self.pan - center * self.zoom;
        let painter = ui.painter_at(canvas);
        painter.rect_filled(canvas, 12.0, Color32::from_rgb(20, 24, 33));
        let zoom = self.zoom;
        for index in 0..map.screens.len() {
            let screen = &map.screens[index];
            let rect = Rect::from_min_size(
                origin + Vec2::new(screen.x as f32, screen.y as f32) * zoom,
                Vec2::new(screen.width as f32, screen.height as f32) * zoom,
            );
            let card = ui.interact(rect, ui.id().with(screen.peer.to_hex()), Sense::drag());
            let status = self
                .report
                .as_ref()
                .and_then(|r| r.peers.iter().find(|p| p.peer == screen.peer));
            let connected = status.is_some_and(|p| p.connected);
            let target = self
                .report
                .as_ref()
                .is_some_and(|r| r.target == Some(screen.peer));
            let color = if target {
                Color32::from_rgb(61, 167, 130)
            } else if connected {
                Color32::from_rgb(86, 137, 226)
            } else {
                Color32::GRAY
            };
            painter.rect_filled(rect, 8.0, Color32::from_rgb(37, 44, 58));
            painter.rect_stroke(rect, 8.0, Stroke::new(2.0, color), egui::StrokeKind::Inside);
            painter.text(
                rect.center() - Vec2::new(0.0, 14.0),
                egui::Align2::CENTER_CENTER,
                &screen.name,
                egui::FontId::proportional(16.0),
                Color32::WHITE,
            );
            let text = if status.is_none_or(|s| !s.trusted) {
                "Não pareado"
            } else if !connected {
                "Offline"
            } else if status.is_some_and(|s| !s.ready) {
                match status.map(|s| s.input_status.as_str()) {
                    Some("permissions-required") => "Permissões necessárias",
                    Some("locked" | "session-unavailable") => "Sessão bloqueada",
                    Some("paused") => "Compartilhamento pausado",
                    _ => "Aguardando disponibilidade",
                }
            } else if target {
                "Recebendo controle"
            } else {
                "Conectado"
            };
            painter.text(
                rect.center() + Vec2::new(0.0, 14.0),
                egui::Align2::CENTER_CENTER,
                text,
                egui::FontId::proportional(12.0),
                color,
            );
            if card.drag_started() {
                self.drag_start = Some((index, screen.x, screen.y));
            }
            if card.dragged()
                && let Some((i, x, y)) = self.drag_start.filter(|(i, _, _)| *i == index)
            {
                let delta = card.total_drag_delta().unwrap_or_default() / zoom;
                map.screens[i].x = x + delta.x.round() as i32;
                map.screens[i].y = y + delta.y.round() as i32;
                self.dirty = true;
            }
            if card.drag_stopped() {
                snap(map, index, (14.0 / zoom) as i32);
                self.drag_start = None;
            }
        }
        // Highlight positive-length shared edge segments only.
        for (i, a) in map.screens.iter().enumerate() {
            for b in &map.screens[..i] {
                let y0 = a.y.max(b.y);
                let y1 = (a.y + a.height).min(b.y + b.height);
                if y1 > y0 && (a.x + a.width == b.x || b.x + b.width == a.x) {
                    let x = if a.x < b.x { b.x } else { a.x };
                    painter.line_segment(
                        [
                            origin + Vec2::new(x as f32, y0 as f32) * zoom,
                            origin + Vec2::new(x as f32, y1 as f32) * zoom,
                        ],
                        Stroke::new(5.0, Color32::LIGHT_GREEN),
                    );
                }
                let x0 = a.x.max(b.x);
                let x1 = (a.x + a.width).min(b.x + b.width);
                if x1 > x0 && (a.y + a.height == b.y || b.y + b.height == a.y) {
                    let y = if a.y < b.y { b.y } else { a.y };
                    painter.line_segment(
                        [
                            origin + Vec2::new(x0 as f32, y as f32) * zoom,
                            origin + Vec2::new(x1 as f32, y as f32) * zoom,
                        ],
                        Stroke::new(5.0, Color32::LIGHT_GREEN),
                    );
                }
            }
        }
    }
}
fn snap(map: &mut DesktopMap, index: usize, tolerance: i32) {
    let a = map.screens[index].clone();
    let mut dx = tolerance + 1;
    let mut dy = tolerance + 1;
    for (i, b) in map.screens.iter().enumerate() {
        if i == index {
            continue;
        }
        for d in [b.x + b.width - a.x, b.x - a.x - a.width] {
            if d.abs() < dx.abs() {
                dx = d;
            }
        }
        for d in [b.y + b.height - a.y, b.y - a.y - a.height, b.y - a.y] {
            if d.abs() < dy.abs() {
                dy = d;
            }
        }
    }
    if dx.abs() <= tolerance {
        map.screens[index].x += dx;
    }
    if dy.abs() <= tolerance {
        map.screens[index].y += dy;
    }
    if opendesk_core::desktop_map::validate(map).is_err() {
        map.screens[index] = a;
    }
}
impl eframe::App for MapApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let focus = self.focus.metadata().ok().and_then(|m| m.modified().ok());
        if focus != self.last_focus {
            self.last_focus = focus;
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::Focus);
        }
        ui.style_mut()
            .text_styles
            .insert(egui::TextStyle::Body, egui::FontId::proportional(16.0));
        ui.style_mut()
            .text_styles
            .insert(egui::TextStyle::Button, egui::FontId::proportional(15.0));
        ui.style_mut()
            .text_styles
            .insert(egui::TextStyle::Heading, egui::FontId::proportional(25.0));
        self.receive();
        if self.last_poll.elapsed() > Duration::from_secs(1) && !self.busy {
            self.last_poll = Instant::now();
            self.send(IpcRequest::MapGet);
        }
        ui.ctx().request_repaint_after(Duration::from_millis(100));
        egui::Frame::new().inner_margin(20.0).show(ui, |ui| {
            self.toolbar(ui);
            ui.add_space(16.0);
            self.canvas(ui);
        });
        let mut adding = self.adding;
        egui::Window::new("Adicionar computador")
            .open(&mut adding)
            .show(ui.ctx(), |ui| {
                ui.label("O PIN confirma a confiança entre os computadores.");
                for peer in self.discover.clone() {
                    ui.horizontal(|ui| {
                        ui.label(&peer.name);
                        if !peer.paired && ui.button("Parear").clicked() {
                            self.send(IpcRequest::Pair { name: peer.name });
                        }
                    });
                }
                if self.pin_required {
                    ui.text_edit_singleline(&mut self.pin);
                    if ui.button("Confirmar PIN").clicked() {
                        self.send(IpcRequest::SubmitPin {
                            pin: self.pin.clone(),
                        });
                        self.pin.clear();
                        self.pin_required = false;
                    }
                }
                if let (Some(report), Some(map)) = (&self.report, &mut self.draft) {
                    for peer in &report.peers {
                        if !map.screens.iter().any(|s| s.peer == peer.peer)
                            && ui
                                .button(format!("Incluir {} no mapa", peer.name))
                                .clicked()
                        {
                            let x = map.screens.iter().map(|s| s.x + s.width).max().unwrap_or(0);
                            map.screens.push(Screen {
                                peer: peer.peer,
                                name: peer.name.clone(),
                                x,
                                y: 0,
                                width: 1920,
                                height: 1080,
                            });
                            self.dirty = true;
                        }
                    }
                }
            });
        self.adding = adding;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use opendesk_proto::control::PeerId;
    #[test]
    fn dragged_card_stays_at_the_accumulated_position_when_pointer_stops() {
        let (requests, _) = mpsc::channel();
        let (_, responses) = mpsc::channel();
        let revision = Revision {
            counter: 1,
            author: PeerId([1; 16]),
        };
        let map = DesktopMap {
            group: revision.author,
            revision,
            screens: (0..2)
                .map(|i| Screen {
                    peer: PeerId([i as u8; 16]),
                    name: format!("{i}"),
                    x: i * 1000,
                    y: 0,
                    width: 1000,
                    height: 600,
                })
                .collect(),
        };
        let Ok(focus) = std::fs::File::open("/dev/null") else {
            return;
        };
        let mut app = MapApp {
            focus,
            last_focus: None,
            requests,
            responses,
            report: None,
            draft: Some(map),
            base: Some(revision),
            dirty: false,
            message: String::new(),
            zoom: 0.14,
            pan: Vec2::ZERO,
            discover: vec![],
            adding: false,
            pin: String::new(),
            pin_required: false,
            last_poll: Instant::now(),
            busy: false,
            drag_start: None,
        };
        let ctx = egui::Context::default();
        let frame = |app: &mut MapApp, events: Vec<egui::Event>, time: f64| {
            let mut output = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(Rect::from_min_size(
                        egui::Pos2::ZERO,
                        Vec2::new(900.0, 600.0),
                    )),
                    time: Some(time),
                    events,
                    ..Default::default()
                },
                |ui| app.canvas(ui),
            );
            output.textures_delta.clear();
        };
        frame(&mut app, vec![], 0.0);
        let start = egui::pos2(380.0, 300.0);
        frame(
            &mut app,
            vec![
                egui::Event::PointerMoved(start),
                egui::Event::PointerButton {
                    pos: start,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
            0.1,
        );
        frame(
            &mut app,
            vec![egui::Event::PointerMoved(start + egui::vec2(0.0, 70.0))],
            0.2,
        );
        let moved = app.draft.as_ref().map(|m| m.screens[0].y);
        assert!(moved.is_some_and(|y| y > 0));
        frame(&mut app, vec![], 0.3);
        assert_eq!(app.draft.as_ref().map(|m| m.screens[0].y), moved);
    }
}
