use opendesk_proto::control::{OutputGeometry, Side};

pub const THICKNESS: u32 = 1;

pub fn along_edge_global(side: Side, origin: (i32, i32), local: (f64, f64)) -> f64 {
    if side.is_horizontal() {
        f64::from(origin.1) + local.1
    } else {
        f64::from(origin.0) + local.0
    }
}

pub fn along_edge_local(side: Side, origin: (i32, i32), global: f64) -> (f64, f64) {
    let across = f64::from(THICKNESS) / 2.0;
    if side.is_horizontal() {
        (across, global - f64::from(origin.1))
    } else {
        (global - f64::from(origin.0), across)
    }
}

pub fn edge_point(side: Side, output: &OutputGeometry, along: f64) -> (f64, f64) {
    let last_x = f64::from(output.x + output.width - 1);
    let last_y = f64::from(output.y + output.height - 1);
    let along_x = along.clamp(f64::from(output.x), last_x);
    let along_y = along.clamp(f64::from(output.y), last_y);
    match side {
        Side::Left => (f64::from(output.x), along_y),
        Side::Right => (last_x, along_y),
        Side::Top => (along_x, f64::from(output.y)),
        Side::Bottom => (along_x, last_y),
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
    fn global_coordinate_follows_the_output_origin() {
        assert_eq!(
            along_edge_global(Side::Right, (1920, 100), (0.5, 300.0)),
            400.0
        );
        assert_eq!(along_edge_global(Side::Left, (-1920, 0), (0.0, 12.5)), 12.5);
        assert_eq!(
            along_edge_global(Side::Top, (1920, 100), (640.0, 0.0)),
            2560.0
        );
        assert_eq!(along_edge_global(Side::Bottom, (0, 0), (33.0, 0.5)), 33.0);
    }

    #[test]
    fn local_coordinate_inverts_global() {
        for side in [Side::Left, Side::Right, Side::Top, Side::Bottom] {
            let origin = (1920, 100);
            let local = along_edge_local(side, origin, 700.0);
            assert_eq!(along_edge_global(side, origin, local), 700.0);
        }
        assert_eq!(
            along_edge_local(Side::Right, (1920, 100), 400.0),
            (0.5, 300.0)
        );
        assert_eq!(
            along_edge_local(Side::Top, (1920, 100), 2560.0),
            (640.0, 0.5)
        );
    }

    #[test]
    fn edge_point_lands_on_the_last_pixel_of_the_side() {
        assert_eq!(edge_point(Side::Right, &output(), 400.0), (4479.0, 400.0));
        assert_eq!(edge_point(Side::Left, &output(), 400.0), (1920.0, 400.0));
        assert_eq!(edge_point(Side::Top, &output(), 2000.0), (2000.0, 100.0));
        assert_eq!(
            edge_point(Side::Bottom, &output(), 2000.0),
            (2000.0, 1539.0)
        );
    }

    #[test]
    fn edge_point_clamps_to_the_output() {
        assert_eq!(edge_point(Side::Right, &output(), 5.0), (4479.0, 100.0));
        assert_eq!(edge_point(Side::Right, &output(), 9999.0), (4479.0, 1539.0));
    }
}
