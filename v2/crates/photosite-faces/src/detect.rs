//! Turning YuNet's twelve output tensors into rectangles.
//!
//! The model does not hand back faces. It hands back, for three grids of
//! different coarseness, a score per cell, a box measured from that cell's
//! own corner, and five landmarks measured the same way. Reading that is
//! arithmetic and belongs here rather than inside the network: it is the
//! half of the detector that can be tested without a model on disk, and it
//! is also the half that is easy to get subtly wrong — an off-by-one in the
//! grid puts every face eight pixels up and to the left, which looks almost
//! right.
//!
//! OpenCV does this inside `FaceDetectorYN`, which is why v1 never had to.
//! The numbers here are that code's, checked against it.

/// The three grids the model predicts on, coarsest cell last.
pub const STRIDES: [usize; 3] = [8, 16, 32];

/// Below this a detection is not offered at all. v1's value, and the one the
/// model's own documentation suggests.
pub const SCORE_THRESHOLD: f32 = 0.8;

/// How much two boxes may overlap before the weaker is taken to be the same
/// face seen twice.
pub const NMS_THRESHOLD: f32 = 0.3;

/// One face as the model sees it, in pixels of the square canvas it was
/// given.
#[derive(Debug, Clone, PartialEq)]
pub struct Raw {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    pub score: f32,
    pub landmarks: [(f32, f32); 5],
}

/// The four tensors one grid produces, flattened.
#[derive(Debug, Clone, Copy)]
pub struct Grid<'a> {
    pub stride: usize,
    /// Is there a face here at all, and is it a face — two separate heads,
    /// and the model means them multiplied.
    pub classification: &'a [f32],
    pub objectness: &'a [f32],
    /// Four numbers a cell: the centre as an offset within the cell, and the
    /// width and height as logarithms.
    pub boxes: &'a [f32],
    /// Ten numbers a cell: five landmarks as offsets within the cell.
    pub landmarks: &'a [f32],
}

/// Reads every grid and keeps what clears the score threshold.
///
/// `canvas` is the side of the square the model was given, which is what the
/// cell counts are worked out from.
pub fn decode(grids: &[Grid<'_>], canvas: usize, score_threshold: f32) -> Vec<Raw> {
    let mut found = Vec::new();
    for grid in grids {
        let columns = canvas / grid.stride;
        let rows = canvas / grid.stride;
        let cells = columns * rows;
        if grid.classification.len() < cells
            || grid.objectness.len() < cells
            || grid.boxes.len() < cells * 4
            || grid.landmarks.len() < cells * 10
        {
            // A tensor of the wrong size means the model is not the one this
            // code was written for. Reading it anyway would produce faces
            // where there are none.
            tracing::warn!(
                stride = grid.stride,
                "the detector's output is not the shape this build expects"
            );
            continue;
        }

        for row in 0..rows {
            for column in 0..columns {
                let cell = row * columns + column;
                // Both heads are probabilities and the model means their
                // geometric mean; clamping first is OpenCV's, and it matters
                // because a raw head can come back a hair outside 0..1 and
                // the square root of a negative is not a score.
                let classification = grid.classification[cell].clamp(0.0, 1.0);
                let objectness = grid.objectness[cell].clamp(0.0, 1.0);
                let score = (classification * objectness).sqrt();
                if score < score_threshold {
                    continue;
                }

                let stride = grid.stride as f32;
                let at = cell * 4;
                let centre_x = (column as f32 + grid.boxes[at]) * stride;
                let centre_y = (row as f32 + grid.boxes[at + 1]) * stride;
                let width = grid.boxes[at + 2].exp() * stride;
                let height = grid.boxes[at + 3].exp() * stride;

                let mut landmarks = [(0f32, 0f32); 5];
                for (index, landmark) in landmarks.iter_mut().enumerate() {
                    let at = cell * 10 + index * 2;
                    *landmark = (
                        (column as f32 + grid.landmarks[at]) * stride,
                        (row as f32 + grid.landmarks[at + 1]) * stride,
                    );
                }

                found.push(Raw {
                    x: centre_x - width / 2.0,
                    y: centre_y - height / 2.0,
                    width,
                    height,
                    score,
                    landmarks,
                });
            }
        }
    }

    found
}

/// Keeps the strongest of every group of boxes describing one face.
///
/// The three grids see the same face at three coarsenesses and each offers
/// its own rectangle. Without this a face in the middle of a photograph
/// arrives three times, is embedded three times, and turns up three times in
/// the group waiting to be named.
pub fn suppress(mut faces: Vec<Raw>, threshold: f32) -> Vec<Raw> {
    faces.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    let mut kept: Vec<Raw> = Vec::with_capacity(faces.len());
    for face in faces {
        if kept.iter().any(|other| overlap(&face, other) > threshold) {
            continue;
        }

        kept.push(face);
    }

    kept
}

/// Intersection over union. Two rectangles of the same face come out near 1,
/// two people standing apart near 0.
pub fn overlap(first: &Raw, second: &Raw) -> f32 {
    let left = first.x.max(second.x);
    let top = first.y.max(second.y);
    let right = (first.x + first.width).min(second.x + second.width);
    let bottom = (first.y + first.height).min(second.y + second.height);
    if right <= left || bottom <= top {
        return 0.0;
    }

    let intersection = (right - left) * (bottom - top);
    let union = first.width * first.height + second.width * second.height - intersection;
    if union <= 0.0 {
        0.0
    } else {
        intersection / union
    }
}

/// The same, for two rectangles held as fractions of their photograph. The
/// expression backfill uses it to recognise a face it has already stored.
pub fn overlap_of(first: (f64, f64, f64, f64), second: (f64, f64, f64, f64)) -> f64 {
    let left = first.0.max(second.0);
    let top = first.1.max(second.1);
    let right = (first.0 + first.2).min(second.0 + second.2);
    let bottom = (first.1 + first.3).min(second.1 + second.3);
    if right <= left || bottom <= top {
        return 0.0;
    }

    let intersection = (right - left) * (bottom - top);
    let union = first.2 * first.3 + second.2 * second.3 - intersection;
    if union <= 0.0 {
        0.0
    } else {
        intersection / union
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Builds the four tensors of one grid with a single cell lit up.
    fn one_face(
        stride: usize,
        canvas: usize,
        at: (usize, usize),
    ) -> (Vec<f32>, Vec<f32>, Vec<f32>, Vec<f32>) {
        let cells = (canvas / stride) * (canvas / stride);
        let columns = canvas / stride;
        let cell = at.1 * columns + at.0;
        let mut classification = vec![0.0; cells];
        let mut objectness = vec![0.0; cells];
        let mut boxes = vec![0.0; cells * 4];
        let mut landmarks = vec![0.0; cells * 10];
        classification[cell] = 1.0;
        objectness[cell] = 1.0;
        // Centred on the cell, four cells wide and tall.
        boxes[cell * 4] = 0.5;
        boxes[cell * 4 + 1] = 0.5;
        boxes[cell * 4 + 2] = 4f32.ln();
        boxes[cell * 4 + 3] = 4f32.ln();
        for index in 0..5 {
            landmarks[cell * 10 + index * 2] = 0.5;
            landmarks[cell * 10 + index * 2 + 1] = 0.5;
        }

        (classification, objectness, boxes, landmarks)
    }

    #[test]
    fn a_cell_becomes_a_rectangle_where_that_cell_is() {
        let (cls, obj, boxes, kps) = one_face(8, 640, (10, 20));
        let faces = decode(
            &[Grid {
                stride: 8,
                classification: &cls,
                objectness: &obj,
                boxes: &boxes,
                landmarks: &kps,
            }],
            640,
            SCORE_THRESHOLD,
        );

        assert_eq!(faces.len(), 1);
        let face = &faces[0];
        // Cell (10, 20) at stride 8, centre half a cell in, four cells wide.
        assert!((face.x - (10.5 * 8.0 - 16.0)).abs() < 1e-3, "{face:?}");
        assert!((face.y - (20.5 * 8.0 - 16.0)).abs() < 1e-3, "{face:?}");
        assert!((face.width - 32.0).abs() < 1e-3, "{face:?}");
        assert!((face.landmarks[0].0 - 84.0).abs() < 1e-3, "{face:?}");
    }

    /// The rows-then-columns order is the one thing here that cannot be
    /// checked by eye, and getting it backwards puts every face in the
    /// mirrored place along the diagonal.
    #[test]
    fn the_grid_is_read_rows_first() {
        let (cls, obj, boxes, kps) = one_face(32, 640, (1, 5));
        let faces = decode(
            &[Grid {
                stride: 32,
                classification: &cls,
                objectness: &obj,
                boxes: &boxes,
                landmarks: &kps,
            }],
            640,
            SCORE_THRESHOLD,
        );
        let face = &faces[0];
        let centre_x = face.x + face.width / 2.0;
        let centre_y = face.y + face.height / 2.0;
        assert!((centre_x - 1.5 * 32.0).abs() < 1e-3, "{face:?}");
        assert!((centre_y - 5.5 * 32.0).abs() < 1e-3, "{face:?}");
    }

    #[test]
    fn a_weak_detection_is_never_offered() {
        let (mut cls, obj, boxes, kps) = one_face(8, 640, (3, 3));
        cls[3 * 80 + 3] = 0.1;
        let faces = decode(
            &[Grid {
                stride: 8,
                classification: &cls,
                objectness: &obj,
                boxes: &boxes,
                landmarks: &kps,
            }],
            640,
            SCORE_THRESHOLD,
        );
        assert!(faces.is_empty());
    }

    #[test]
    fn a_score_outside_the_range_does_not_become_a_square_root_of_nothing() {
        let (mut cls, mut obj, boxes, kps) = one_face(8, 640, (3, 3));
        cls[3 * 80 + 3] = 1.2;
        obj[3 * 80 + 3] = -0.4;
        let faces = decode(
            &[Grid {
                stride: 8,
                classification: &cls,
                objectness: &obj,
                boxes: &boxes,
                landmarks: &kps,
            }],
            640,
            0.0,
        );
        assert!(faces.iter().all(|face| face.score.is_finite()));
    }

    #[test]
    fn a_tensor_of_the_wrong_size_is_refused_rather_than_read_anyway() {
        let faces = decode(
            &[Grid {
                stride: 8,
                classification: &[1.0],
                objectness: &[1.0],
                boxes: &[0.0; 4],
                landmarks: &[0.0; 10],
            }],
            640,
            SCORE_THRESHOLD,
        );
        assert!(faces.is_empty());
    }

    fn raw(x: f32, y: f32, side: f32, score: f32) -> Raw {
        Raw {
            x,
            y,
            width: side,
            height: side,
            score,
            landmarks: [(0.0, 0.0); 5],
        }
    }

    /// One face found by all three grids has to come back once.
    #[test]
    fn one_face_seen_three_times_is_one_face() {
        let kept = suppress(
            vec![
                raw(100.0, 100.0, 40.0, 0.9),
                raw(102.0, 101.0, 41.0, 0.95),
                raw(99.0, 98.0, 39.0, 0.85),
            ],
            NMS_THRESHOLD,
        );
        assert_eq!(kept.len(), 1);
        assert!((kept[0].score - 0.95).abs() < 1e-6, "the strongest is kept");
    }

    #[test]
    fn two_people_standing_apart_stay_two_people() {
        let kept = suppress(
            vec![raw(0.0, 0.0, 40.0, 0.9), raw(300.0, 300.0, 40.0, 0.9)],
            NMS_THRESHOLD,
        );
        assert_eq!(kept.len(), 2);
    }

    #[test]
    fn boxes_that_do_not_touch_do_not_overlap() {
        assert_eq!(
            overlap(&raw(0.0, 0.0, 10.0, 1.0), &raw(50.0, 50.0, 10.0, 1.0)),
            0.0
        );
        assert!((overlap(&raw(0.0, 0.0, 10.0, 1.0), &raw(0.0, 0.0, 10.0, 1.0)) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn the_normalised_overlap_agrees_with_the_pixel_one() {
        let pixels = overlap(&raw(10.0, 10.0, 20.0, 1.0), &raw(20.0, 10.0, 20.0, 1.0));
        let fractions = overlap_of((0.1, 0.1, 0.2, 0.2), (0.2, 0.1, 0.2, 0.2));
        assert!((f64::from(pixels) - fractions).abs() < 1e-6);
    }
}
