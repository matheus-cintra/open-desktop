use opendesk_core::desktop_map::{resolve, validate};
use opendesk_proto::{
    control::{PeerId, Side},
    map::{DesktopMap, Revision, Screen},
};
fn id(n: u8) -> PeerId {
    PeerId([n; 16])
}
fn map() -> DesktopMap {
    DesktopMap {
        group: id(1),
        revision: Revision {
            counter: 1,
            author: id(1),
        },
        screens: (1..=3)
            .map(|n| Screen {
                peer: id(n),
                name: format!("{n}"),
                x: (n as i32 - 1) * 100,
                y: 0,
                width: 100,
                height: 100,
            })
            .collect(),
    }
}
#[test]
fn horizontal_vertical_partial_alignment_and_offline_ray() {
    let mut m = map();
    assert_eq!(
        resolve(&m, id(1), Side::Right, 0.5, |_| true).unwrap().peer,
        id(2)
    );
    assert_eq!(
        resolve(&m, id(1), Side::Right, 0.5, |p| p != id(2))
            .unwrap()
            .peer,
        id(3)
    );
    assert_eq!(
        resolve(&m, id(3), Side::Left, 0.5, |p| p != id(2))
            .unwrap()
            .peer,
        id(1)
    );
    m.screens[2].y = 75;
    assert!(resolve(&m, id(1), Side::Right, 0.5, |p| p != id(2)).is_none());
    let c = resolve(&m, id(1), Side::Right, 0.9, |p| p != id(2)).unwrap();
    assert!((c.fraction - 0.15).abs() < 0.0001);
    for s in &mut m.screens {
        std::mem::swap(&mut s.x, &mut s.y);
    }
    assert_eq!(
        resolve(&m, id(1), Side::Bottom, 0.9, |p| p != id(2))
            .unwrap()
            .peer,
        id(3)
    );
    assert_eq!(
        resolve(&m, id(3), Side::Top, 0.15, |p| p != id(2))
            .unwrap()
            .peer,
        id(1)
    );
}
#[test]
fn no_corner_overlap_nonfinite_fraction_or_coordinate_overflow() {
    let mut m = map();
    m.screens[1].y = 100;
    assert!(validate(&m).is_err());
    let mut m = map();
    m.screens[1].x = 99;
    assert!(validate(&m).is_err());
    let mut m = map();
    m.screens[0].x = i32::MIN;
    assert!(validate(&m).is_err());
    let m = map();
    for fraction in [f32::NAN, f32::INFINITY, -0.1, 1.0] {
        assert!(resolve(&m, id(1), Side::Right, fraction, |_| true).is_none());
    }
}
