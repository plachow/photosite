//! Turning a photograph by a quarter, without touching a pixel.
//!
//! A JPEG turned the obvious way — decoded, rotated, encoded again — loses a
//! little every time, and somebody sorting a card of portraits turns dozens
//! of them. The EXIF orientation tag exists so that nothing has to be
//! re-encoded: the pixels stay as the sensor read them, and every reader
//! turns them on the way to the screen. Explorer, every browser, Lightroom
//! and this application all honour it, so changing the tag *is* turning the
//! photograph, as far as anybody looking at it can tell.
//!
//! The eight values are the eight ways of laying a rectangle down: four
//! turns, each with or without a mirror. Adding a turn to one of them is a
//! composition, not an addition — turning a mirrored photograph clockwise
//! turns what is stored the other way — and getting that wrong turns a
//! selfie the wrong way round. So the arithmetic lives here, with its tests.
//!
//! Face frames are fractions of the photograph the right way up, and when
//! that turns, they have to turn with it or every name ends up beside the
//! face it belongs to.

/// The orientation after turning a photograph `quarters` quarter-turns
/// clockwise, as it is shown. Negative is anticlockwise.
///
/// Anything outside 1..=8 is read as 1, the way the reader treats it: a
/// photograph whose tag makes no sense is shown as it is stored, and a turn
/// starts from there.
pub fn turned(orientation: u8, quarters: i32) -> u8 {
    let (turns, mirrored) = parts(orientation);
    // Turning after a mirror is mirroring after turning the other way, so a
    // mirrored photograph takes the turn with its sign reversed.
    let by = if mirrored { -quarters } else { quarters };
    compose((turns as i32 + by).rem_euclid(4) as u8, mirrored)
}

/// Whether the tag says to mirror the photograph as well as turn it.
pub fn is_mirrored(orientation: u8) -> bool {
    parts(orientation).1
}

/// The tag as clockwise quarter-turns of what is stored, followed by an
/// optional mirror across the vertical axis.
fn parts(orientation: u8) -> (u8, bool) {
    match orientation {
        2 => (0, true),
        3 => (2, false),
        4 => (2, true),
        5 => (1, true),
        6 => (1, false),
        7 => (3, true),
        8 => (3, false),
        _ => (0, false),
    }
}

fn compose(turns: u8, mirrored: bool) -> u8 {
    match (turns % 4, mirrored) {
        (0, false) => 1,
        (0, true) => 2,
        (1, false) => 6,
        (1, true) => 5,
        (2, false) => 3,
        (2, true) => 4,
        (3, false) => 8,
        (3, true) => 7,
        _ => unreachable!("turns is taken modulo four"),
    }
}

/// A rectangle given as fractions of the photograph — left, top, width,
/// height — after the photograph turns `quarters` quarter-turns clockwise.
pub fn turned_rect(x: f64, y: f64, width: f64, height: f64, quarters: i32) -> [f64; 4] {
    match quarters.rem_euclid(4) {
        // What was the top edge is now the right-hand one.
        1 => [1.0 - y - height, x, height, width],
        2 => [1.0 - x - width, 1.0 - y - height, width, height],
        // And the other way: what was the left edge is now the bottom.
        3 => [y, 1.0 - x - width, height, width],
        _ => [x, y, width, height],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_right_turn_goes_round_the_four_upright_orientations() {
        assert_eq!(turned(1, 1), 6);
        assert_eq!(turned(6, 1), 3);
        assert_eq!(turned(3, 1), 8);
        assert_eq!(turned(8, 1), 1);
    }

    #[test]
    fn a_left_turn_goes_round_them_the_other_way() {
        assert_eq!(turned(1, -1), 8);
        assert_eq!(turned(8, -1), 3);
        assert_eq!(turned(3, -1), 6);
        assert_eq!(turned(6, -1), 1);
    }

    #[test]
    fn four_turns_either_way_come_back_to_where_they_started() {
        for orientation in 1..=8 {
            let mut right = orientation;
            let mut left = orientation;
            for _ in 0..4 {
                right = turned(right, 1);
                left = turned(left, -1);
            }
            assert_eq!(right, orientation, "four right turns from {orientation}");
            assert_eq!(left, orientation, "four left turns from {orientation}");
        }
    }

    #[test]
    fn a_turn_and_its_opposite_undo_each_other() {
        for orientation in 1..=8 {
            assert_eq!(turned(turned(orientation, 1), -1), orientation);
            assert_eq!(turned(turned(orientation, 2), 2), orientation);
        }
    }

    /// The one that is easy to get wrong. A photograph mirrored across the
    /// vertical axis and then turned right is the stored one turned left and
    /// then mirrored — which is what 7 says.
    #[test]
    fn a_mirrored_photograph_turns_and_stays_mirrored() {
        assert_eq!(turned(2, 1), 7);
        assert_eq!(turned(2, -1), 5);
        assert_eq!(turned(4, 2), 2);
        for orientation in 1..=8 {
            assert_eq!(
                is_mirrored(turned(orientation, 1)),
                is_mirrored(orientation),
                "{orientation}"
            );
        }
    }

    #[test]
    fn a_tag_that_makes_no_sense_turns_from_upright() {
        assert_eq!(turned(0, 1), 6);
        assert_eq!(turned(77, -1), 8);
    }

    fn close(a: [f64; 4], b: [f64; 4]) -> bool {
        a.iter().zip(b).all(|(a, b)| (a - b).abs() < 1e-9)
    }

    /// A face in the top left corner of a landscape frame is in the top right
    /// once the frame is turned right, and it is as tall as it was wide.
    #[test]
    fn a_face_frame_follows_the_photograph_round() {
        let face = (0.1, 0.2, 0.3, 0.1);
        assert!(close(
            turned_rect(face.0, face.1, face.2, face.3, 1),
            [0.7, 0.1, 0.1, 0.3]
        ));
        assert!(close(
            turned_rect(face.0, face.1, face.2, face.3, -1),
            [0.2, 0.6, 0.1, 0.3]
        ));
        assert!(close(
            turned_rect(face.0, face.1, face.2, face.3, 2),
            [0.6, 0.7, 0.3, 0.1]
        ));
    }

    #[test]
    fn a_face_frame_turned_all_the_way_round_is_where_it_was() {
        let mut face = [0.12, 0.34, 0.2, 0.25];
        for _ in 0..4 {
            face = turned_rect(face[0], face[1], face[2], face[3], 1);
        }
        assert!(close(face, [0.12, 0.34, 0.2, 0.25]), "{face:?}");
    }
}
