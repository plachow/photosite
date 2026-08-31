//! Is this face smiling, and are the eyes open.
//!
//! Two more networks over the crop the recogniser already got, so the
//! expensive part — finding the face and putting it straight — is paid once.
//!
//! Both are **optional**, and that shapes everything here. A face scanned
//! before the models were on disk keeps its identity and simply carries no
//! score; a later pass scores it in place. So a failure in either network
//! costs one number and never a face, and the whole of this module returns
//! options rather than errors.

use crate::align::{ALIGNED, REFERENCE};
use crate::{EMOTION, EYE_STATE, planes};
use anyhow::{Context, Result};
use std::path::Path;
use tract_onnx::prelude::*;

type Net = std::sync::Arc<TypedRunnableModel>;

/// FER+ reads a 64x64 grey face.
const EMOTION_SIDE: usize = 64;

/// Happiness is the second of FER+'s eight emotions: neutral, happiness,
/// surprise, sadness, anger, disgust, fear, contempt.
const HAPPINESS: usize = 1;

/// The eye model reads a 32x32 patch. On a 112-pixel face that wraps an eye
/// with room for the lid and the brow, which is what it needs to tell a
/// blink from a glance.
const EYE_PATCH: usize = 32;

/// Above this a face counts as smiling, and its eyes as open.
///
/// The same number serves both, and it is deliberately the obvious one: a
/// probability over a half. Anything cleverer would need a corpus to tune
/// against, and the badge it drives is a hint to look, not a verdict.
pub const THRESHOLD: f64 = 0.5;

pub struct Nets {
    emotion: Net,
    eyes: Net,
}

impl std::fmt::Debug for Nets {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Nets")
    }
}

impl Nets {
    pub fn load(directory: &Path) -> Result<Self> {
        Ok(Self {
            emotion: load(&directory.join(EMOTION), [1, 1, EMOTION_SIDE, EMOTION_SIDE])?,
            eyes: load(&directory.join(EYE_STATE), [1, 3, EYE_PATCH, EYE_PATCH])?,
        })
    }

    /// Both scores for one aligned face. Either may be missing on its own:
    /// a network that fails costs its own number and not the other's.
    pub fn score(&self, aligned: &[u8]) -> (Option<f64>, Option<f64>) {
        let smile = match self.smile(aligned) {
            Ok(value) => Some(value),
            Err(error) => {
                tracing::debug!(%error, "the smile could not be read");
                None
            }
        };
        let eyes = match self.eyes_open(aligned) {
            Ok(value) => Some(value),
            Err(error) => {
                tracing::debug!(%error, "the eyes could not be read");
                None
            }
        };
        (smile, eyes)
    }

    /// FER+ over the grey face; "smiling" is the softmax probability of its
    /// happiness class. The network wants raw 0..255 values, not a
    /// normalised image.
    fn smile(&self, aligned: &[u8]) -> Result<f64> {
        let grey = grey_square(aligned, EMOTION_SIDE);
        let tensor = tract_ndarray::Array4::from_shape_vec(
            (1, 1, EMOTION_SIDE, EMOTION_SIDE),
            grey.iter().map(|value| f32::from(*value)).collect(),
        )?;
        let output = self.emotion.run(tvec!(Tensor::from(tensor).into()))?;
        let logits = crate::values(&output[0])
            .context("the emotion network's answer is not a vector of numbers")?;
        let probabilities = softmax(logits);
        probabilities
            .get(HAPPINESS)
            .copied()
            .context("the emotion network answered with too few classes")
    }

    /// The eye model over a patch around each eye of the aligned face.
    ///
    /// **The face keeps its weaker eye.** "Eyes open" has to mean both of
    /// them, or the one photograph in the set where somebody blinked with
    /// one eye passes the filter that exists precisely to catch it.
    fn eyes_open(&self, aligned: &[u8]) -> Result<f64> {
        let mut weakest = 1.0f64;
        for (x, y) in [REFERENCE[0], REFERENCE[1]] {
            let left = ((x.round() as i64) - (EYE_PATCH as i64) / 2)
                .clamp(0, (ALIGNED - EYE_PATCH) as i64) as usize;
            let top = ((y.round() as i64) - (EYE_PATCH as i64) / 2)
                .clamp(0, (ALIGNED - EYE_PATCH) as i64) as usize;

            let mut patch = vec![0u8; EYE_PATCH * EYE_PATCH * 3];
            for row in 0..EYE_PATCH {
                let from = ((top + row) * ALIGNED + left) * 3;
                let to = row * EYE_PATCH * 3;
                patch[to..to + EYE_PATCH * 3].copy_from_slice(&aligned[from..from + EYE_PATCH * 3]);
            }

            // The model's documented preprocessing: (pixel - 127) / 255.
            let tensor = planes(&patch, EYE_PATCH, EYE_PATCH, |value| {
                (value - 127.0) / 255.0
            });
            let output = self.eyes.run(tvec!(tensor.into()))?;
            let answer = crate::values(&output[0])
                .context("the eye network's answer is not a vector of numbers")?;
            // The network normalises internally: [closed, open] sum to one.
            let open = f64::from(
                *answer
                    .get(1)
                    .context("the eye network answered with too few classes")?,
            );
            weakest = weakest.min(open);
        }

        Ok(weakest)
    }
}

fn load(path: &Path, shape: [usize; 4]) -> Result<Net> {
    anyhow::ensure!(path.is_file(), "{} is not there", path.display());
    tract_onnx::onnx()
        .model_for_path(path)?
        .with_input_fact(0, f32::fact(shape).into())?
        .into_optimized()?
        .into_runnable()
}

/// The aligned face as one grey square of the given side.
///
/// The weights are OpenCV's `BGR2GRAY`, which is what the model was trained
/// through. A plain average of the three channels would be a different
/// image, and a face lit from one side is exactly where the difference
/// shows.
pub(crate) fn grey_square(aligned: &[u8], side: usize) -> Vec<u8> {
    let mut out = vec![0u8; side * side];
    for y in 0..side {
        // Nearest neighbour is enough here: the input is already a small,
        // upright, tightly cropped face, and the network's own first layer
        // is a blur.
        let source_y = y * ALIGNED / side;
        for x in 0..side {
            let source_x = x * ALIGNED / side;
            let at = (source_y * ALIGNED + source_x) * 3;
            let value = 0.299 * f32::from(aligned[at])
                + 0.587 * f32::from(aligned[at + 1])
                + 0.114 * f32::from(aligned[at + 2]);
            out[y * side + x] = value.round().clamp(0.0, 255.0) as u8;
        }
    }

    out
}

/// Logits into probabilities, with the largest taken off first so a big
/// logit does not overflow the exponential.
pub(crate) fn softmax(logits: &[f32]) -> Vec<f64> {
    let largest = logits.iter().copied().fold(f32::NEG_INFINITY, f32::max);
    let exponentials: Vec<f64> = logits
        .iter()
        .map(|value| f64::from(*value - largest).exp())
        .collect();
    let total: f64 = exponentials.iter().sum();
    if total <= 0.0 {
        return vec![0.0; logits.len()];
    }

    exponentials
        .into_iter()
        .map(|value| value / total)
        .collect()
}

/// How one photograph's faces scored, taken together.
///
/// Two counts and not one ratio, because "nobody has been scored yet" and
/// "everybody was scored and everybody is smiling" are different states and
/// a ratio cannot tell them apart.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Summary {
    pub faces: usize,
    /// How many of them the expression models have actually seen.
    pub scored: usize,
    pub smiling: usize,
    pub eyes_open: usize,
}

impl Summary {
    /// Everybody on the photograph is scored and smiling.
    pub fn all_smiling(&self) -> bool {
        self.faces > 0 && self.smiling == self.faces
    }

    pub fn all_eyes_open(&self) -> bool {
        self.faces > 0 && self.eyes_open == self.faces
    }

    /// Somebody who was looked at is not smiling. Deliberately measured
    /// against `scored` and not `faces`: a face nobody has scored is not
    /// evidence of a frown.
    pub fn anyone_not_smiling(&self) -> bool {
        self.smiling < self.scored
    }

    pub fn anyone_blinking(&self) -> bool {
        self.eyes_open < self.scored
    }

    /// Is there anything here worth a badge on a tile?
    pub fn worth_showing(&self) -> bool {
        self.anyone_not_smiling() || self.anyone_blinking()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn probabilities_add_up_to_one() {
        let out = softmax(&[1.0, 2.0, 3.0]);
        assert!((out.iter().sum::<f64>() - 1.0).abs() < 1e-9);
        assert!(out[2] > out[1] && out[1] > out[0]);
    }

    /// A logit large enough to overflow the exponential is not far-fetched;
    /// a network fed a black square can produce one.
    #[test]
    fn a_huge_logit_does_not_become_a_not_a_number() {
        let out = softmax(&[1000.0, 0.0]);
        assert!(out.iter().all(|value| value.is_finite()), "{out:?}");
        assert!((out[0] - 1.0).abs() < 1e-9);
    }

    #[test]
    fn equal_logits_are_equally_likely() {
        let out = softmax(&[5.0, 5.0, 5.0, 5.0]);
        assert!(out.iter().all(|value| (value - 0.25).abs() < 1e-9));
    }

    #[test]
    fn grey_follows_the_weights_the_model_was_trained_through() {
        let red = [255u8, 0, 0].repeat(ALIGNED * ALIGNED);
        let grey = grey_square(&red, 4);
        assert_eq!(grey[0], 76); // 0.299 * 255
    }

    #[test]
    fn a_photograph_nobody_has_scored_is_not_evidence_of_a_frown() {
        let summary = Summary {
            faces: 3,
            scored: 0,
            smiling: 0,
            eyes_open: 0,
        };
        assert!(!summary.anyone_not_smiling());
        assert!(!summary.anyone_blinking());
        assert!(!summary.worth_showing());
    }

    #[test]
    fn one_blink_among_four_is_worth_showing() {
        let summary = Summary {
            faces: 4,
            scored: 4,
            smiling: 4,
            eyes_open: 3,
        };
        assert!(summary.anyone_blinking());
        assert!(!summary.anyone_not_smiling());
        assert!(summary.worth_showing());
        assert!(summary.all_smiling());
        assert!(!summary.all_eyes_open());
    }

    /// "Everybody is smiling" has to mean everybody, including the faces
    /// nobody has scored. Otherwise a photograph half of whose faces predate
    /// the models passes a filter it should not.
    #[test]
    fn all_smiling_means_every_face_and_not_every_scored_face() {
        let summary = Summary {
            faces: 4,
            scored: 2,
            smiling: 2,
            eyes_open: 2,
        };
        assert!(!summary.all_smiling());
        assert!(!summary.all_eyes_open());
    }

    #[test]
    fn a_photograph_with_no_faces_passes_nothing() {
        let empty = Summary::default();
        assert!(!empty.all_smiling());
        assert!(!empty.all_eyes_open());
        assert!(!empty.worth_showing());
    }
}
