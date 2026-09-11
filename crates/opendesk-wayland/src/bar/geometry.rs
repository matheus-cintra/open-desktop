use opendesk_proto::control::{OutputGeometry, Side};
use smithay_client_toolkit::shell::wlr_layer::Anchor;

pub const THICKNESS: u32 = 4;
pub const ARRIVAL_LENGTH: u32 = 220;
const PROGRESS_BASE_LENGTH: f32 = 40.0;
const PROGRESS_GROWTH_LENGTH: f32 = 180.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BarPlacement {
    pub anchor: Anchor,
    pub margin_top: i32,
    pub margin_left: i32,
    pub width: u32,
    pub height: u32,
}

impl BarPlacement {
    pub fn size(&self) -> (u32, u32) {
        (self.width, self.height)
    }
}

pub fn progress_length(progress: f32) -> u32 {
    let clamped = progress.clamp(0.0, 1.0);
    (PROGRESS_BASE_LENGTH + clamped * PROGRESS_GROWTH_LENGTH).round() as u32
}

pub fn place_bar(side: Side, position: f64, output: &OutputGeometry, length: u32) -> BarPlacement {
    let (origin, extent) = if side.is_horizontal() {
        (output.y, output.height)
    } else {
        (output.x, output.width)
    };
    let extent = u32::try_from(extent).unwrap_or(0);
    let length = length.min(extent).max(1);
    let local = position - f64::from(origin);
    let max_start = extent.saturating_sub(length);
    let start = (local - f64::from(length) / 2.0)
        .round()
        .clamp(0.0, f64::from(max_start)) as i32;
    match side {
        Side::Left => BarPlacement {
            anchor: Anchor::LEFT | Anchor::TOP,
            margin_top: start,
            margin_left: 0,
            width: THICKNESS,
            height: length,
        },
        Side::Right => BarPlacement {
            anchor: Anchor::RIGHT | Anchor::TOP,
            margin_top: start,
            margin_left: 0,
            width: THICKNESS,
            height: length,
        },
        Side::Top => BarPlacement {
            anchor: Anchor::TOP | Anchor::LEFT,
            margin_top: 0,
            margin_left: start,
            width: length,
            height: THICKNESS,
        },
        Side::Bottom => BarPlacement {
            anchor: Anchor::BOTTOM | Anchor::LEFT,
            margin_top: 0,
            margin_left: start,
            width: length,
            height: THICKNESS,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn output() -> OutputGeometry {
        OutputGeometry {
            name: "DP-1".to_owned(),
            x: 1920,
            y: 100,
            width: 2560,
            height: 1440,
        }
    }

    #[test]
    fn progress_length_grows_from_40_to_220() {
        assert_eq!(progress_length(0.0), 40);
        assert_eq!(progress_length(0.3), 94);
        assert_eq!(progress_length(0.9), 202);
        assert_eq!(progress_length(1.0), 220);
        assert_eq!(progress_length(1.7), 220);
        assert_eq!(progress_length(-0.5), 40);
    }

    #[test]
    fn vertical_sides_center_on_y_with_a_top_margin() {
        let left = place_bar(Side::Left, 820.0, &output(), 220);
        assert_eq!(
            left,
            BarPlacement {
                anchor: Anchor::LEFT | Anchor::TOP,
                margin_top: 610,
                margin_left: 0,
                width: THICKNESS,
                height: 220,
            }
        );
        let right = place_bar(Side::Right, 820.0, &output(), 94);
        assert_eq!(right.anchor, Anchor::RIGHT | Anchor::TOP);
        assert_eq!(right.margin_top, 673);
        assert_eq!(right.size(), (THICKNESS, 94));
    }

    #[test]
    fn horizontal_sides_center_on_x_with_a_left_margin() {
        let top = place_bar(Side::Top, 3200.0, &output(), 220);
        assert_eq!(
            top,
            BarPlacement {
                anchor: Anchor::TOP | Anchor::LEFT,
                margin_top: 0,
                margin_left: 1170,
                width: 220,
                height: THICKNESS,
            }
        );
        let bottom = place_bar(Side::Bottom, 3200.0, &output(), 40);
        assert_eq!(bottom.anchor, Anchor::BOTTOM | Anchor::LEFT);
        assert_eq!(bottom.margin_left, 1260);
        assert_eq!(bottom.size(), (40, THICKNESS));
    }

    #[test]
    fn placement_is_clamped_inside_the_output() {
        assert_eq!(place_bar(Side::Right, 105.0, &output(), 220).margin_top, 0);
        assert_eq!(
            place_bar(Side::Right, 1535.0, &output(), 220).margin_top,
            1220
        );
        assert_eq!(place_bar(Side::Top, 1925.0, &output(), 220).margin_left, 0);
        assert_eq!(
            place_bar(Side::Bottom, 4475.0, &output(), 220).margin_left,
            2340
        );
        let tiny = OutputGeometry {
            width: 100,
            ..output()
        };
        let oversized = place_bar(Side::Top, 1950.0, &tiny, 220);
        assert_eq!(oversized.margin_left, 0);
        assert_eq!(oversized.width, 100);
    }
}
