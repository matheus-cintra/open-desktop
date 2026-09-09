use opendesk_core::edge::{fraction_along, point_at, position_at, total_length};
use opendesk_core::layout::EdgeSegment;
use opendesk_proto::control::Side;

fn segment(start: i32, end: i32, coordinate: i32) -> EdgeSegment {
    EdgeSegment {
        output: "output".to_owned(),
        start,
        end,
        coordinate,
        covers_whole_output_edge: true,
    }
}

fn split_edge() -> Vec<EdgeSegment> {
    vec![segment(0, 200, 2560), segment(1280, 1440, 2560)]
}

#[test]
fn total_length_sums_segments() {
    assert_eq!(total_length(&split_edge()), 360);
    assert_eq!(total_length(&[]), 0);
}

#[test]
fn fraction_is_computed_over_the_concatenated_segments() {
    let segments = split_edge();
    assert_eq!(fraction_along(&segments, 0.0), Some(0.0));
    assert_eq!(fraction_along(&segments, 180.0), Some(0.5));
    assert_eq!(fraction_along(&segments, 1280.0), Some(200.0 / 360.0));
    assert_eq!(fraction_along(&segments, 1440.0), Some(1.0));
    assert_eq!(fraction_along(&segments, 700.0), None);
    assert_eq!(fraction_along(&segments, -1.0), None);
    assert_eq!(fraction_along(&[], 10.0), None);
}

#[test]
fn position_and_fraction_round_trip() {
    let segments = split_edge();
    for position in [0.0, 50.0, 199.0, 1280.0, 1400.0, 1440.0] {
        let fraction = fraction_along(&segments, position).unwrap();
        let back = position_at(&segments, fraction).unwrap();
        assert!(
            (back - position).abs() < 0.01,
            "{position} -> {fraction} -> {back}"
        );
    }
}

#[test]
fn position_at_clamps_fraction() {
    let segments = split_edge();
    assert_eq!(position_at(&segments, -1.0), Some(0.0));
    assert_eq!(position_at(&segments, 2.0), Some(1440.0));
    assert_eq!(position_at(&[], 0.5), None);
}

#[test]
fn point_at_moves_inward_per_side() {
    let vertical = vec![segment(0, 1000, 500)];
    assert_eq!(
        point_at(&vertical, Side::Right, 0.5, 2.0),
        Some((498.0, 500.0))
    );
    assert_eq!(
        point_at(&vertical, Side::Left, 0.5, 2.0),
        Some((502.0, 500.0))
    );
    let horizontal = vec![segment(0, 1000, 700)];
    assert_eq!(
        point_at(&horizontal, Side::Top, 0.25, 3.0),
        Some((250.0, 703.0))
    );
    assert_eq!(
        point_at(&horizontal, Side::Bottom, 0.25, 3.0),
        Some((250.0, 697.0))
    );
    assert_eq!(point_at(&[], Side::Bottom, 0.25, 3.0), None);
}

#[test]
fn point_at_uses_the_coordinate_of_the_segment_it_lands_on() {
    let segments = vec![segment(0, 200, 2560), segment(200, 1280, 4480)];
    assert_eq!(
        point_at(&segments, Side::Right, 0.0, 0.0),
        Some((2560.0, 0.0))
    );
    assert_eq!(
        point_at(&segments, Side::Right, 1.0, 0.0),
        Some((4480.0, 1280.0))
    );
}
