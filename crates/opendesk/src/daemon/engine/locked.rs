use std::time::{Duration, Instant};

use opendesk_core::session::{SessionEvent, SessionState};
use opendesk_proto::control::Side;
use tokio::sync::mpsc;
use tracing::{debug, warn};

use super::Engine;
use super::monitor::{LockState, MAX_AGE, MonitorEvent, Query, Request, Sample};

const MOTION_AGE: Duration = Duration::from_millis(200);

pub struct MonitorState {
    pub lock: LockState,
    pub generation: u64,
    valid_lock: Option<Instant>,
    valid_cursor: Instant,
    next_lock: Instant,
    next_cursor: Instant,
    pending: bool,
    outward: Option<(Instant, Instant)>,
}

impl MonitorState {
    pub fn new(now: Instant) -> Self {
        Self {
            lock: LockState::Unknown,
            generation: 0,
            valid_lock: None,
            valid_cursor: now,
            next_lock: now,
            next_cursor: now,
            pending: false,
            outward: None,
        }
    }

    pub fn invalidate(&mut self, now: Instant) {
        self.generation = self.generation.wrapping_add(1);
        self.outward = None;
        self.next_lock = now;
        self.next_cursor = now;
        self.valid_cursor = now;
    }

    pub fn known(&self, now: Instant) -> bool {
        self.lock != LockState::Unknown
            && self
                .valid_lock
                .is_some_and(|at| now.saturating_duration_since(at) < MAX_AGE)
    }

    fn confirms_motion(&self, started: Instant, now: Instant) -> bool {
        self.outward.is_some_and(|(first, last)| {
            first <= started && now.saturating_duration_since(last) <= MOTION_AGE
        })
    }
}

impl Engine {
    pub(super) fn poll_compositor(&mut self, now: Instant, sender: &mpsc::Sender<Request>) {
        let controlled_locked =
            self.session.is_controlled() && self.monitor.lock == LockState::Locked;
        let stale_cursor = controlled_locked
            && now.saturating_duration_since(self.monitor.valid_cursor) >= MAX_AGE;
        if !self.monitor.known(now) || stale_cursor {
            if self.monitor.lock != LockState::Unknown {
                warn!("compositor monitor unavailable for one second");
            }
            self.monitor.lock = LockState::Unknown;
            if !matches!(self.session.state(), SessionState::Idle) || self.has_file_drag() {
                self.recover_compositor("compositor monitor unavailable");
            }
        }
        if self.monitor.pending {
            return;
        }
        let active = !matches!(self.session.state(), SessionState::Idle);
        let query = if now >= self.monitor.next_lock {
            self.monitor.next_lock = now + Duration::from_millis(if active { 100 } else { 500 });
            Query::Lock
        } else if controlled_locked && now >= self.monitor.next_cursor {
            self.monitor.next_cursor = now + Duration::from_millis(50);
            Query::Cursor
        } else {
            return;
        };
        let request = Request {
            generation: self.monitor.generation,
            started: now,
            query,
        };
        self.monitor.pending = sender.try_send(request).is_ok();
    }

    pub(super) fn on_monitor(&mut self, event: MonitorEvent) {
        self.monitor.pending = false;
        let (request, sample) = match event {
            MonitorEvent::Sample(request, sample) => (request, sample),
            MonitorEvent::Unavailable(request) => {
                if request.generation == self.monitor.generation {
                    debug!(query = ?request.query, "compositor query unavailable");
                }
                return;
            }
        };
        let now = Instant::now();
        if request.generation != self.monitor.generation
            || now.saturating_duration_since(request.started) >= MAX_AGE
        {
            return;
        }
        match sample {
            Sample::Lock(locked) => {
                let lock = if locked {
                    LockState::Locked
                } else {
                    LockState::Unlocked
                };
                if self.monitor.lock != lock {
                    debug!(?lock, "compositor lock state changed");
                    self.monitor.outward = None;
                    self.monitor.valid_cursor = now;
                    self.monitor.next_cursor = now;
                }
                self.monitor.lock = lock;
                self.monitor.valid_lock = Some(request.started);
                if locked
                    && (self.has_file_drag()
                        || (!matches!(self.session.state(), SessionState::Idle)
                            && !self.session.is_controlled()))
                {
                    self.recover_compositor("local compositor locked while capturing or dragging");
                }
            }
            Sample::Cursor { x, y } => {
                self.monitor.valid_cursor = request.started;
                if self.monitor.lock != LockState::Locked
                    || !self.monitor.confirms_motion(request.started, now)
                {
                    return;
                }
                let Some(side) = self.session.return_side() else {
                    return;
                };
                if !self.session.can_return_at(side, now) {
                    return;
                }
                let Some(fraction) = self.edges.cursor_fraction(side, x, y) else {
                    return;
                };
                debug!(%side, "locked destination returning through entry edge");
                self.dispatch(SessionEvent::EdgeEntered {
                    side,
                    fraction,
                    peer: None,
                });
            }
        }
    }

    pub(super) fn locked_motion(&mut self, dx: f64, dy: f64) {
        let now = Instant::now();
        let Some(side) = self.session.return_side() else {
            return;
        };
        if self.monitor.lock != LockState::Locked || !self.session.can_return_at(side, now) {
            self.monitor.outward = None;
            return;
        }
        let outward = match side {
            Side::Left => -dx,
            Side::Right => dx,
            Side::Top => -dy,
            Side::Bottom => dy,
        };
        self.monitor.outward = if outward > 0.0 && dx.is_finite() && dy.is_finite() {
            Some((
                self.monitor
                    .outward
                    .filter(|(_, last)| now.saturating_duration_since(*last) <= MOTION_AGE)
                    .map_or(now, |(first, _)| first),
                now,
            ))
        } else {
            None
        };
    }

    fn has_file_drag(&self) -> bool {
        self.pending_drag.is_some()
            || self.pending_transfer.is_some()
            || self.incoming_drag.is_some()
            || self.outgoing_drag.is_some()
            || self.active_drop.is_some()
            || self.receiving_transfer.is_some()
            || self.authorized_return_drop.is_some()
    }

    fn recover_compositor(&mut self, cause: &'static str) {
        warn!(cause, "releasing control and cancelling file drag");
        self.pending_transfer = None;
        self.dispatch(SessionEvent::Disabled);
    }
}

#[cfg(test)]
mod tests;
