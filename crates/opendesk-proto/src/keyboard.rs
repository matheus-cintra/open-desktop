//! Physical keys use the USB HID keyboard usage page, never XKB modifier masks.
const KEYS: &[(u16, u32)] = &[
    (4, 30),
    (5, 48),
    (6, 46),
    (7, 32),
    (8, 18),
    (9, 33),
    (10, 34),
    (11, 35),
    (12, 23),
    (13, 36),
    (14, 37),
    (15, 38),
    (16, 50),
    (17, 49),
    (18, 24),
    (19, 25),
    (20, 16),
    (21, 19),
    (22, 31),
    (23, 20),
    (24, 22),
    (25, 47),
    (26, 17),
    (27, 45),
    (28, 21),
    (29, 44),
    (30, 2),
    (31, 3),
    (32, 4),
    (33, 5),
    (34, 6),
    (35, 7),
    (36, 8),
    (37, 9),
    (38, 10),
    (39, 11),
    (40, 28),
    (41, 1),
    (42, 14),
    (43, 15),
    (44, 57),
    (45, 12),
    (46, 13),
    (47, 26),
    (48, 27),
    (49, 43),
    (51, 39),
    (52, 40),
    (53, 41),
    (54, 51),
    (55, 52),
    (56, 53),
    (57, 58),
    (58, 59),
    (59, 60),
    (60, 61),
    (61, 62),
    (62, 63),
    (63, 64),
    (64, 65),
    (65, 66),
    (66, 67),
    (67, 68),
    (68, 87),
    (69, 88),
    (70, 99),
    (71, 70),
    (72, 119),
    (73, 110),
    (74, 102),
    (75, 104),
    (76, 111),
    (77, 107),
    (78, 109),
    (79, 106),
    (80, 105),
    (81, 108),
    (82, 103),
    (83, 69),
    (84, 98),
    (85, 55),
    (86, 74),
    (87, 78),
    (88, 96),
    (89, 79),
    (90, 80),
    (91, 81),
    (92, 75),
    (93, 76),
    (94, 77),
    (95, 71),
    (96, 72),
    (97, 73),
    (98, 82),
    (99, 83),
    (100, 86),
    (101, 127),
    (103, 117),
    (135, 89),
    (137, 124),
    (224, 29),
    (225, 42),
    (226, 56),
    (227, 125),
    (228, 97),
    (229, 54),
    (230, 100),
    (231, 126),
];
pub fn evdev_to_hid(code: u32) -> Option<u16> {
    KEYS.iter().find(|(_, e)| *e == code).map(|(h, _)| *h)
}
pub fn hid_to_evdev(usage: u16) -> Option<u32> {
    KEYS.iter().find(|(h, _)| *h == usage).map(|(_, e)| *e)
}
pub fn swap_control_super(usage: u16) -> u16 {
    match usage {
        224 => 227,
        227 => 224,
        228 => 231,
        231 => 228,
        other => other,
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn physical_keys_roundtrip() {
        for &(hid, evdev) in KEYS {
            assert_eq!(evdev_to_hid(evdev), Some(hid));
            assert_eq!(hid_to_evdev(hid), Some(evdev));
        }
        assert_eq!(evdev_to_hid(9999), None);
    }
    #[test]
    fn modifier_swap_is_involutive_and_preserves_alt_shift() {
        for usage in 0..256 {
            assert_eq!(swap_control_super(swap_control_super(usage)), usage);
        }
        assert_eq!(swap_control_super(224), 227);
        assert_eq!(swap_control_super(226), 226);
    }
}
