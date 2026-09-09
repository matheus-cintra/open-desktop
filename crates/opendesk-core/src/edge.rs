use opendesk_proto::control::Side;

use crate::layout::EdgeSegment;

pub fn total_length(segments: &[EdgeSegment]) -> i32 {
    segments
        .iter()
        .map(|segment| segment.end - segment.start)
        .sum()
}

pub fn fraction_along(segments: &[EdgeSegment], position: f64) -> Option<f32> {
    let total = f64::from(total_length(segments));
    if total <= 0.0 {
        return None;
    }
    let mut offset = 0.0;
    for segment in segments {
        let start = f64::from(segment.start);
        let end = f64::from(segment.end);
        if position >= start && position <= end {
            let fraction = (offset + position - start) / total;
            return Some(fraction.clamp(0.0, 1.0) as f32);
        }
        offset += end - start;
    }
    None
}

struct Located<'segments> {
    segment: &'segments EdgeSegment,
    along: f64,
}

fn locate(segments: &[EdgeSegment], fraction: f32) -> Option<Located<'_>> {
    let total = f64::from(total_length(segments));
    if total <= 0.0 {
        return None;
    }
    let target = f64::from(fraction.clamp(0.0, 1.0)) * total;
    let mut offset = 0.0;
    let mut last = None;
    for segment in segments {
        let length = f64::from(segment.end - segment.start);
        if length <= 0.0 {
            continue;
        }
        if target <= offset + length {
            return Some(Located {
                segment,
                along: f64::from(segment.start) + target - offset,
            });
        }
        offset += length;
        last = Some(segment);
    }
    last.map(|segment| Located {
        segment,
        along: f64::from(segment.end),
    })
}

pub fn position_at(segments: &[EdgeSegment], fraction: f32) -> Option<f64> {
    locate(segments, fraction).map(|located| located.along)
}

pub fn point_at(
    segments: &[EdgeSegment],
    side: Side,
    fraction: f32,
    inset_px: f64,
) -> Option<(f64, f64)> {
    let located = locate(segments, fraction)?;
    let coordinate = f64::from(located.segment.coordinate);
    let point = match side {
        Side::Left => (coordinate + inset_px, located.along),
        Side::Right => (coordinate - inset_px, located.along),
        Side::Top => (located.along, coordinate + inset_px),
        Side::Bottom => (located.along, coordinate - inset_px),
    };
    Some(point)
}
