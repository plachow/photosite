//! Putting the eyes where the network expects them.
//!
//! Every network downstream of the detector — the one that recognises, the
//! one that reads a smile, the one that reads a blink — was trained on faces
//! standing upright with the eyes, the nose and the mouth corners at fixed
//! coordinates. Handing one a face tilted thirty degrees is not a slightly
//! worse answer, it is a different question.
//!
//! So the detector's five landmarks are matched onto five reference points
//! by a **similarity transform**: turn, uniform scale and shift, and nothing
//! else. Not a full affine — a face is not sheared or stretched by being
//! photographed from a different angle, and letting the fit stretch one
//! would make two photographs of the same person less alike rather than
//! more.

use photosite_image::Rgb;

/// The side of the aligned crop. What SFace was trained on.
pub const ALIGNED: usize = 112;

/// Where the eyes, the nose and the mouth corners belong on the aligned
/// crop — the ArcFace reference, which is what SFace and everything built on
/// it assume.
pub const REFERENCE: [(f32, f32); 5] = [
    (38.2946, 51.6963), // right eye as the photograph sees it, left as the subject does
    (73.5318, 51.5014),
    (56.0252, 71.7366), // nose
    (41.5493, 92.3655), // mouth corners
    (70.7299, 92.2041),
];

/// A turn, a uniform scale and a shift, written as the two rows of a 2x3
/// matrix.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Similarity {
    pub a: f32,
    pub b: f32,
    pub tx: f32,
    pub c: f32,
    pub d: f32,
    pub ty: f32,
}

impl Similarity {
    pub fn apply(&self, x: f32, y: f32) -> (f32, f32) {
        (
            self.a * x + self.b * y + self.tx,
            self.c * x + self.d * y + self.ty,
        )
    }

    /// The transform that undoes this one. The warp works backwards — for
    /// every pixel of the output it asks where in the source that pixel came
    /// from — which is the only way to fill an output without holes.
    pub fn inverse(&self) -> Option<Self> {
        let determinant = self.a * self.d - self.b * self.c;
        if determinant.abs() < 1e-9 {
            return None;
        }

        let (a, b, c, d) = (
            self.d / determinant,
            -self.b / determinant,
            -self.c / determinant,
            self.a / determinant,
        );
        Some(Self {
            a,
            b,
            tx: -(a * self.tx + b * self.ty),
            c,
            d,
            ty: -(c * self.tx + d * self.ty),
        })
    }
}

/// The best similarity transform taking `from` onto `to`, in the
/// least-squares sense.
///
/// This is Umeyama's closed form, and it is used rather than the iterative
/// estimator v1 got from OpenCV for one reason: with five points there is
/// nothing for a robust estimator to be robust against. Five landmarks are
/// either all right or the face is not a face, and a randomised algorithm
/// over five points only buys a different answer on a different run — which
/// would mean the same photograph scanned twice producing two embeddings.
pub fn similarity(from: &[(f32, f32)], to: &[(f32, f32)]) -> Option<Similarity> {
    let count = from.len();
    if count < 2 || count != to.len() {
        return None;
    }

    let n = count as f64;
    let mean = |points: &[(f32, f32)]| {
        let sum = points.iter().fold((0f64, 0f64), |acc, p| {
            (acc.0 + p.0 as f64, acc.1 + p.1 as f64)
        });
        (sum.0 / n, sum.1 / n)
    };
    let (from_cx, from_cy) = mean(from);
    let (to_cx, to_cy) = mean(to);

    // The covariance between the two centred sets, and how spread out the
    // source is. The rotation comes out of the first, the scale out of both.
    let (mut sxx, mut sxy, mut syx, mut syy, mut variance) = (0f64, 0f64, 0f64, 0f64, 0f64);
    for (source, target) in from.iter().zip(to) {
        let (sx, sy) = (source.0 as f64 - from_cx, source.1 as f64 - from_cy);
        let (tx, ty) = (target.0 as f64 - to_cx, target.1 as f64 - to_cy);
        sxx += tx * sx;
        sxy += tx * sy;
        syx += ty * sx;
        syy += ty * sy;
        variance += sx * sx + sy * sy;
    }

    if variance < 1e-12 {
        // Every landmark in one spot. Not a face, and not something to
        // divide by.
        return None;
    }

    // For a similarity the rotation reduces to one angle, and the two
    // diagonals of the covariance give its cosine and sine directly.
    let cos = sxx + syy;
    let sin = syx - sxy;
    let norm = (cos * cos + sin * sin).sqrt();
    if norm < 1e-12 {
        return None;
    }

    let scale = norm / variance;
    let (cos, sin) = (cos / norm, sin / norm);
    let (a, b) = (scale * cos, -scale * sin);
    let (c, d) = (scale * sin, scale * cos);
    Some(Similarity {
        a: a as f32,
        b: b as f32,
        tx: (to_cx - (a * from_cx + b * from_cy)) as f32,
        c: c as f32,
        d: d as f32,
        ty: (to_cy - (c * from_cx + d * from_cy)) as f32,
    })
}

/// Warps the face out of the frame onto the 112x112 the networks read.
///
/// Bilinear, and sampling outside the frame gives black rather than the
/// nearest edge pixel: a face at the very edge of a photograph is partly
/// missing, and smearing the last column across the gap invents a cheek that
/// was never photographed.
pub fn warp(frame: &Rgb, landmarks: &[(f32, f32); 5]) -> Option<Vec<u8>> {
    let transform = similarity(landmarks, &REFERENCE)?;
    let inverse = transform.inverse()?;
    let mut out = vec![0u8; ALIGNED * ALIGNED * 3];
    for y in 0..ALIGNED {
        for x in 0..ALIGNED {
            let (sx, sy) = inverse.apply(x as f32 + 0.5, y as f32 + 0.5);
            let pixel = sample(frame, sx - 0.5, sy - 0.5);
            let at = (y * ALIGNED + x) * 3;
            out[at..at + 3].copy_from_slice(&pixel);
        }
    }

    Some(out)
}

/// One bilinear sample. Outside the frame is black.
fn sample(frame: &Rgb, x: f32, y: f32) -> [u8; 3] {
    let (width, height) = (frame.width as i64, frame.height as i64);
    if width == 0 || height == 0 {
        return [0, 0, 0];
    }

    let x0 = x.floor();
    let y0 = y.floor();
    let fx = x - x0;
    let fy = y - y0;
    let (x0, y0) = (x0 as i64, y0 as i64);

    let mut out = [0u8; 3];
    for (channel, value) in out.iter_mut().enumerate() {
        let mut total = 0f32;
        for (dx, dy, weight) in [
            (0, 0, (1.0 - fx) * (1.0 - fy)),
            (1, 0, fx * (1.0 - fy)),
            (0, 1, (1.0 - fx) * fy),
            (1, 1, fx * fy),
        ] {
            let (px, py) = (x0 + dx, y0 + dy);
            if px < 0 || py < 0 || px >= width || py >= height {
                continue;
            }

            let at = ((py as usize * frame.width as usize) + px as usize) * 3 + channel;
            total += weight * f32::from(frame.pixels[at]);
        }

        *value = total.round().clamp(0.0, 255.0) as u8;
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(width: u32, height: u32, fill: impl Fn(u32, u32) -> [u8; 3]) -> Rgb {
        let mut pixels = Vec::with_capacity((width * height * 3) as usize);
        for y in 0..height {
            for x in 0..width {
                pixels.extend_from_slice(&fill(x, y));
            }
        }

        Rgb::new(width, height, pixels).unwrap()
    }

    #[test]
    fn a_face_already_in_place_is_left_where_it_is() {
        let transform = similarity(&REFERENCE, &REFERENCE).unwrap();
        for (x, y) in REFERENCE {
            let (mapped_x, mapped_y) = transform.apply(x, y);
            assert!((mapped_x - x).abs() < 1e-3, "{mapped_x} vs {x}");
            assert!((mapped_y - y).abs() < 1e-3, "{mapped_y} vs {y}");
        }
    }

    /// The whole point: a face photographed at an angle and half the size
    /// comes back upright and the right size.
    #[test]
    fn a_turned_and_shrunken_face_is_put_straight() {
        let angle = 0.4f32;
        let (sin, cos) = angle.sin_cos();
        let turned: Vec<(f32, f32)> = REFERENCE
            .iter()
            .map(|(x, y)| {
                let (x, y) = (x - 56.0, y - 56.0);
                (
                    0.5 * (cos * x - sin * y) + 200.0,
                    0.5 * (sin * x + cos * y) + 300.0,
                )
            })
            .collect();

        let transform = similarity(&turned, &REFERENCE).unwrap();
        for (source, target) in turned.iter().zip(REFERENCE) {
            let (x, y) = transform.apply(source.0, source.1);
            assert!((x - target.0).abs() < 1e-2, "{x} vs {}", target.0);
            assert!((y - target.1).abs() < 1e-2, "{y} vs {}", target.1);
        }
    }

    /// A similarity may not stretch one axis. If it could, two photographs
    /// of one person taken from different angles would be warped toward each
    /// other and the embedding would stop meaning anything.
    #[test]
    fn the_fit_refuses_to_stretch_one_axis() {
        let stretched: Vec<(f32, f32)> = REFERENCE.iter().map(|(x, y)| (x * 2.0, *y)).collect();
        let transform = similarity(&stretched, &REFERENCE).unwrap();
        let horizontal = (transform.a * transform.a + transform.c * transform.c).sqrt();
        let vertical = (transform.b * transform.b + transform.d * transform.d).sqrt();
        assert!(
            (horizontal - vertical).abs() < 1e-4,
            "scale differs by axis: {horizontal} vs {vertical}"
        );
    }

    #[test]
    fn the_inverse_undoes_the_transform() {
        let transform = Similarity {
            a: 0.5,
            b: -0.2,
            tx: 30.0,
            c: 0.2,
            d: 0.5,
            ty: -12.0,
        };
        let back = transform.inverse().unwrap();
        let (x, y) = transform.apply(17.0, -4.0);
        let (x, y) = back.apply(x, y);
        assert!((x - 17.0).abs() < 1e-3, "{x}");
        assert!((y + 4.0).abs() < 1e-3, "{y}");
    }

    #[test]
    fn a_degenerate_set_of_landmarks_is_refused_rather_than_divided_by() {
        let all_in_one_place = [(5.0, 5.0); 5];
        assert!(similarity(&all_in_one_place, &REFERENCE).is_none());
    }

    #[test]
    fn the_warp_lands_the_landmarks_on_the_reference_points() {
        // A face twice the size and turned, with a mark at each landmark.
        // Where the marks end up is the whole of what the warp promises.
        let angle = 0.3f32;
        let (sin, cos) = angle.sin_cos();
        let place = |x: f32, y: f32| {
            let (x, y) = (x - 56.0, y - 56.0);
            (
                2.0 * (cos * x - sin * y) + 150.0,
                2.0 * (sin * x + cos * y) + 150.0,
            )
        };
        let mut landmarks = [(0f32, 0f32); 5];
        for (index, (x, y)) in REFERENCE.iter().enumerate() {
            landmarks[index] = place(*x, *y);
        }

        let frame = frame(300, 300, |x, y| {
            let near = landmarks
                .iter()
                .any(|(lx, ly)| (x as f32 - lx).abs() < 5.0 && (y as f32 - ly).abs() < 5.0);
            if near { [255, 255, 255] } else { [0, 0, 0] }
        });

        let aligned = warp(&frame, &landmarks).unwrap();
        for (x, y) in REFERENCE {
            let at = ((y.round() as usize) * ALIGNED + x.round() as usize) * 3;
            assert!(
                aligned[at] > 128,
                "the landmark at {x},{y} did not land on the reference point"
            );
        }

        // And nothing else did: a warp that filled the crop with white would
        // pass the check above and mean nothing.
        let white = aligned
            .as_chunks::<3>()
            .0
            .iter()
            .filter(|p| p[0] > 128)
            .count();
        assert!(
            white < ALIGNED * ALIGNED / 4,
            "the whole crop came out white: {white} pixels"
        );
    }

    #[test]
    fn sampling_outside_the_frame_is_black_rather_than_a_smeared_edge() {
        let frame = frame(10, 10, |_, _| [200, 200, 200]);
        assert_eq!(sample(&frame, -5.0, 5.0), [0, 0, 0]);
        assert_eq!(sample(&frame, 5.0, 5.0), [200, 200, 200]);
    }
}
