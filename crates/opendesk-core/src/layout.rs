use opendesk_proto::control::{OutputGeometry, Side};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EdgeSegment {
    pub output: String,
    pub start: i32,
    pub end: i32,
    pub coordinate: i32,
    pub covers_whole_output_edge: bool,
}

pub fn bounding_box(layout: &[OutputGeometry]) -> Option<Rect> {
    let first = layout.first()?;
    let mut left = first.x;
    let mut top = first.y;
    let mut right = first.x + first.width;
    let mut bottom = first.y + first.height;
    for output in layout {
        left = left.min(output.x);
        top = top.min(output.y);
        right = right.max(output.x + output.width);
        bottom = bottom.max(output.y + output.height);
    }
    Some(Rect {
        x: left,
        y: top,
        width: right - left,
        height: bottom - top,
    })
}

struct OutputEdge {
    start: i32,
    end: i32,
    coordinate: i32,
}

fn edge_of(output: &OutputGeometry, side: Side) -> OutputEdge {
    match side {
        Side::Left => OutputEdge {
            start: output.y,
            end: output.y + output.height,
            coordinate: output.x,
        },
        Side::Right => OutputEdge {
            start: output.y,
            end: output.y + output.height,
            coordinate: output.x + output.width,
        },
        Side::Top => OutputEdge {
            start: output.x,
            end: output.x + output.width,
            coordinate: output.y,
        },
        Side::Bottom => OutputEdge {
            start: output.x,
            end: output.x + output.width,
            coordinate: output.y + output.height,
        },
    }
}

fn touching_range(
    output: &OutputGeometry,
    side: Side,
    neighbour: &OutputGeometry,
) -> Option<(i32, i32)> {
    let edge = edge_of(output, side);
    let facing = edge_of(neighbour, side.opposite());
    let touches =
        facing.coordinate == edge.coordinate && facing.start < edge.end && edge.start < facing.end;
    touches.then(|| (facing.start.max(edge.start), facing.end.min(edge.end)))
}

fn subtract_range(ranges: &mut Vec<(i32, i32)>, cut: (i32, i32)) {
    let mut remaining = Vec::with_capacity(ranges.len() + 1);
    for &(start, end) in ranges.iter() {
        if cut.1 <= start || cut.0 >= end {
            remaining.push((start, end));
            continue;
        }
        if start < cut.0 {
            remaining.push((start, cut.0));
        }
        if cut.1 < end {
            remaining.push((cut.1, end));
        }
    }
    *ranges = remaining;
}

pub fn exterior_edges(layout: &[OutputGeometry], side: Side) -> Vec<EdgeSegment> {
    let mut segments = Vec::new();
    for (index, output) in layout.iter().enumerate() {
        let edge = edge_of(output, side);
        let mut ranges = vec![(edge.start, edge.end)];
        for (neighbour_index, neighbour) in layout.iter().enumerate() {
            if neighbour_index == index {
                continue;
            }
            if let Some(cut) = touching_range(output, side, neighbour) {
                subtract_range(&mut ranges, cut);
            }
        }
        for (start, end) in ranges {
            segments.push(EdgeSegment {
                output: output.name.clone(),
                start,
                end,
                coordinate: edge.coordinate,
                covers_whole_output_edge: start == edge.start && end == edge.end,
            });
        }
    }
    segments.sort_by(|left, right| {
        (left.start, left.coordinate, &left.output).cmp(&(
            right.start,
            right.coordinate,
            &right.output,
        ))
    });
    segments
}
