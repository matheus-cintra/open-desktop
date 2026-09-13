use std::collections::HashMap;

use crate::platform::StripSpec;
use opendesk_core::config::Config;
use opendesk_core::edge::{fraction_along, point_at, position_at};
use opendesk_core::layout::{EdgeSegment, exterior_edges};
use opendesk_proto::control::{OutputGeometry, Side};

const ENTRY_INSET_PX: f64 = 2.0;
const SIDES: [Side; 4] = [Side::Left, Side::Right, Side::Top, Side::Bottom];

#[derive(Default)]
pub struct EdgeMap {
    segments: HashMap<Side, Vec<EdgeSegment>>,
}

impl EdgeMap {
    pub fn rebuild(&mut self, outputs: &[OutputGeometry], config: &Config) -> Vec<StripSpec> {
        self.segments.clear();
        for side in SIDES {
            let segments = exterior_edges(outputs, side);
            self.segments.insert(side, segments);
        }
        self.strips(config, None, false)
    }

    pub fn strips(&self, config: &Config, return_side: Option<Side>, map: bool) -> Vec<StripSpec> {
        SIDES
            .into_iter()
            .filter(|side| {
                map || match return_side {
                    Some(return_side) => *side == return_side,
                    None => config.peer_for_side(*side).is_some(),
                }
            })
            .flat_map(|side| {
                self.segments
                    .get(&side)
                    .into_iter()
                    .flatten()
                    .filter(|segment| segment.covers_whole_output_edge)
                    .map(move |segment| StripSpec {
                        side,
                        output: segment.output.clone(),
                    })
            })
            .collect()
    }

    pub fn fraction(&self, side: Side, position: f64) -> Option<f32> {
        fraction_along(self.segments.get(&side)?, position)
    }

    /// Geometry is already logical (including scale/transform from xdg-output).
    pub fn cursor_fraction(&self, side: Side, x: f64, y: f64) -> Option<f32> {
        if !x.is_finite() || !y.is_finite() {
            return None;
        }
        let (across, along) = match side {
            Side::Left | Side::Right => (x, y),
            Side::Top | Side::Bottom => (y, x),
        };
        let segments = self.segments.get(&side)?;
        for segment in segments {
            let coordinate = f64::from(segment.coordinate)
                - if matches!(side, Side::Right | Side::Bottom) {
                    1.0
                } else {
                    0.0
                };
            if across.floor() == coordinate
                && along >= f64::from(segment.start)
                && along < f64::from(segment.end)
            {
                return self.fraction(side, along);
            }
        }
        None
    }

    pub fn hint(&self, side: Side, fraction: f32) -> Option<f64> {
        position_at(self.segments.get(&side)?, fraction)
    }

    pub fn entry_point(&self, side: Side, fraction: f32) -> Option<(f64, f64)> {
        point_at(self.segments.get(&side)?, side, fraction, ENTRY_INSET_PX)
    }
}
