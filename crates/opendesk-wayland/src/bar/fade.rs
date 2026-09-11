use std::time::{Duration, Instant};

pub const ARRIVAL_FADE: Duration = Duration::from_millis(600);

#[derive(Clone, Copy, Debug)]
pub struct Fade {
    started_at: Instant,
    duration: Duration,
}

impl Fade {
    pub fn start(now: Instant, duration: Duration) -> Fade {
        Fade {
            started_at: now,
            duration,
        }
    }

    pub fn alpha_multiplier(&self, now: Instant) -> f32 {
        let elapsed = now.saturating_duration_since(self.started_at);
        if elapsed >= self.duration || self.duration.is_zero() {
            return 0.0;
        }
        1.0 - elapsed.as_secs_f32() / self.duration.as_secs_f32()
    }

    pub fn is_finished(&self, now: Instant) -> bool {
        now.saturating_duration_since(self.started_at) >= self.duration
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn alpha_drops_linearly_and_finishes_at_the_duration() {
        let start = Instant::now();
        let fade = Fade::start(start, ARRIVAL_FADE);
        assert_eq!(fade.alpha_multiplier(start), 1.0);
        let half = fade.alpha_multiplier(start + Duration::from_millis(300));
        assert!((half - 0.5).abs() < 0.001, "half {half}");
        assert!(!fade.is_finished(start + Duration::from_millis(599)));
        assert_eq!(fade.alpha_multiplier(start + ARRIVAL_FADE), 0.0);
        assert!(fade.is_finished(start + ARRIVAL_FADE));
        assert_eq!(fade.alpha_multiplier(start + Duration::from_secs(5)), 0.0);
    }

    #[test]
    fn a_zero_duration_is_finished_immediately() {
        let start = Instant::now();
        let fade = Fade::start(start, Duration::ZERO);
        assert!(fade.is_finished(start));
        assert_eq!(fade.alpha_multiplier(start), 0.0);
    }
}
