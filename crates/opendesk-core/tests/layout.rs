use opendesk_core::layout::{EdgeSegment, Rect, bounding_box, exterior_edges};
use opendesk_proto::control::{OutputGeometry, Side};

fn output(name: &str, x: i32, y: i32, width: i32, height: i32) -> OutputGeometry {
    OutputGeometry {
        name: name.to_owned(),
        x,
        y,
        width,
        height,
    }
}

fn segment(name: &str, start: i32, end: i32, coordinate: i32, whole: bool) -> EdgeSegment {
    EdgeSegment {
        output: name.to_owned(),
        start,
        end,
        coordinate,
        covers_whole_output_edge: whole,
    }
}

#[test]
fn bounding_box_of_empty_layout_is_none() {
    assert_eq!(bounding_box(&[]), None);
}

#[test]
fn single_output_has_all_four_edges_exterior() {
    let layout = [output("DP-1", 0, 0, 2560, 1440)];
    assert_eq!(
        bounding_box(&layout),
        Some(Rect {
            x: 0,
            y: 0,
            width: 2560,
            height: 1440
        })
    );
    assert_eq!(
        exterior_edges(&layout, Side::Left),
        vec![segment("DP-1", 0, 1440, 0, true)]
    );
    assert_eq!(
        exterior_edges(&layout, Side::Right),
        vec![segment("DP-1", 0, 1440, 2560, true)]
    );
    assert_eq!(
        exterior_edges(&layout, Side::Top),
        vec![segment("DP-1", 0, 2560, 0, true)]
    );
    assert_eq!(
        exterior_edges(&layout, Side::Bottom),
        vec![segment("DP-1", 0, 2560, 1440, true)]
    );
}

#[test]
fn side_by_side_outputs_of_different_heights_expose_the_uncovered_part() {
    let layout = [
        output("DP-1", 0, 0, 2560, 1440),
        output("HDMI-A-1", 2560, 200, 1920, 1080),
    ];
    assert_eq!(
        bounding_box(&layout),
        Some(Rect {
            x: 0,
            y: 0,
            width: 4480,
            height: 1440
        })
    );
    assert_eq!(
        exterior_edges(&layout, Side::Right),
        vec![
            segment("DP-1", 0, 200, 2560, false),
            segment("HDMI-A-1", 200, 1280, 4480, true),
            segment("DP-1", 1280, 1440, 2560, false),
        ]
    );
    assert_eq!(
        exterior_edges(&layout, Side::Left),
        vec![segment("DP-1", 0, 1440, 0, true)]
    );
    assert_eq!(
        exterior_edges(&layout, Side::Top),
        vec![
            segment("DP-1", 0, 2560, 0, true),
            segment("HDMI-A-1", 2560, 4480, 200, true),
        ]
    );
    assert_eq!(
        exterior_edges(&layout, Side::Bottom),
        vec![
            segment("DP-1", 0, 2560, 1440, true),
            segment("HDMI-A-1", 2560, 4480, 1280, true),
        ]
    );
}

#[test]
fn stacked_outputs_hide_the_shared_horizontal_edge() {
    let layout = [
        output("top", 0, 0, 1920, 1080),
        output("bottom", 0, 1080, 1920, 1080),
    ];
    assert!(
        exterior_edges(&layout, Side::Bottom)
            .iter()
            .all(|edge| edge.output == "bottom")
    );
    assert_eq!(
        exterior_edges(&layout, Side::Bottom),
        vec![segment("bottom", 0, 1920, 2160, true)]
    );
    assert_eq!(
        exterior_edges(&layout, Side::Top),
        vec![segment("top", 0, 1920, 0, true)]
    );
    assert_eq!(
        exterior_edges(&layout, Side::Left),
        vec![
            segment("top", 0, 1080, 0, true),
            segment("bottom", 1080, 2160, 0, true),
        ]
    );
}

#[test]
fn outputs_with_a_gap_do_not_touch() {
    let layout = [
        output("a", 0, 0, 1000, 1000),
        output("b", 1010, 0, 1000, 1000),
    ];
    assert_eq!(
        exterior_edges(&layout, Side::Right),
        vec![
            segment("a", 0, 1000, 1000, true),
            segment("b", 0, 1000, 2010, true),
        ]
    );
}
