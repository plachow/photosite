//! Finding faces, telling them apart, and reading a smile — on this machine
//! and nowhere else.
//!
//! Four small networks, all of them local files, none of them ours:
//!
//! | | | |
//! |---|---|---|
//! | YuNet | OpenCV Zoo, Apache-2.0 | where the faces are, and their five landmarks |
//! | SFace | OpenCV Zoo, Apache-2.0 | a 128-number description of one face |
//! | FER+ | ONNX Model Zoo, MIT | is this face smiling |
//! | open-closed-eye-0001 | OpenVINO Open Model Zoo, Apache-2.0 | are the eyes open |
//!
//! **Nothing leaves the computer, and nothing needs to.** That is the whole
//! reason for choosing four models one can put in a folder over an API that
//! would do all four jobs better: a photograph of somebody's children is not
//! something to upload in exchange for a convenience.
//!
//! ## Why tract rather than ONNX Runtime
//!
//! `ort` is faster and is what everybody reaches for. It is also a native
//! library that has to be shipped, one build per platform, or downloaded at
//! run time from somewhere. [`tract`](https://github.com/sonos/tract) is
//! pure Rust: it cross-compiles with everything else, needs no C toolchain,
//! and adds nothing to what has to be installed. That is the same trade the
//! rest of this application makes about exiftool, and it is the same answer.
//!
//! The cost is real and worth stating. Measured over a folder of holiday
//! photographs, a thousand-pixel frame costs 45 ms to decode and 75 ms to
//! sweep for faces, plus about 45 ms for every face found — so a photograph
//! with three people in it is a fifth of a second on one thread, where ONNX
//! Runtime would be a fraction of that. A scan is a background sweep
//! somebody starts and walks away from, and it is spread over every core, so
//! the trade lands on the right side.
//!
//! ## Why the detector's canvas is 640 and not v1's 1024
//!
//! v1 asked OpenCV for a 1024-pixel detection canvas and OpenCV reshaped the
//! network to suit. The ONNX file itself declares 640x640 and bakes that
//! into its own reshapes; tract holds it to what it declares. So the canvas
//! is the model's own size.
//!
//! What is kept is the invariant that actually matters — **how much of the
//! frame a face has to fill to be found at all**. v1 refused anything under
//! twenty pixels on a 1024 canvas; that is a fiftieth of the frame, and a
//! fiftieth of the frame is what is refused here too. And the alignment and
//! the embedding read the full detection frame rather than the canvas, so a
//! face found small is still described from every pixel the photograph has
//! of it.

pub mod align;
pub mod cluster;
pub mod detect;
pub mod expression;
pub mod math;

use anyhow::{Context, Result};
use photosite_image::Rgb;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tract_onnx::prelude::*;

/// A network, loaded and ready to be run. tract hands one back behind an
/// `Arc` because running is `&self` on a shared plan — which is also what
/// makes it safe to hold one engine and scan on several threads.
type Net = Arc<TypedRunnableModel>;

/// The square the detector is given. The model's own input size; see the
/// crate documentation for why it is not v1's 1024.
pub const CANVAS: usize = 640;

/// How much of the frame a face must fill to be looked at.
///
/// v1's twenty pixels on a canvas of 1024. Held as a fraction rather than as
/// a pixel count so that the canvas size and the rule stay independent —
/// otherwise changing one silently changes the other, and "the scan stopped
/// finding the children in the background" is not a change anybody would
/// connect to a canvas.
pub const MIN_FACE_FRACTION: f32 = 20.0 / 1024.0;

/// The file names, as the OpenCV Zoo and the ONNX Model Zoo publish them.
/// Unchanged from v1 on purpose: somebody upgrading has already downloaded
/// these, and renaming them would mean downloading a hundred megabytes again
/// to no end.
pub const DETECTOR: &str = "face_detection_yunet_2023mar.onnx";
pub const RECOGNIZER: &str = "face_recognition_sface_2021dec.onnx";
pub const EMOTION: &str = "emotion-ferplus-8.onnx";
pub const EYE_STATE: &str = "open_closed_eye.onnx";

/// What is on disk, and therefore what a scan will be able to do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Availability {
    /// The two that a scan cannot do without.
    pub recognition: bool,
    /// The two optional ones. Without them a scan still finds and names
    /// faces; it simply scores no expressions.
    pub expressions: bool,
    /// Which of the four are missing, so somebody can be told what to fetch
    /// rather than that "the models are missing".
    pub missing: Vec<String>,
}

impl Availability {
    pub fn of(directory: &Path) -> Self {
        let present = |name: &str| directory.join(name).is_file();
        let missing = [DETECTOR, RECOGNIZER, EMOTION, EYE_STATE]
            .into_iter()
            .filter(|name| !present(name))
            .map(str::to_owned)
            .collect();
        Self {
            recognition: present(DETECTOR) && present(RECOGNIZER),
            expressions: present(EMOTION) && present(EYE_STATE),
            missing,
        }
    }
}

/// One face found in one photograph.
///
/// The rectangle is a fraction of the frame, never pixels: a photograph is
/// looked at at half a dozen sizes between the tile and the preview, and a
/// rectangle in pixels would be right at exactly one of them.
#[derive(Debug, Clone, PartialEq)]
pub struct Found {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
    pub confidence: f64,
    /// The 128 numbers that say who this is. Normalised, so comparing two is
    /// a dot product.
    pub embedding: Vec<f32>,
    /// `None` when the optional models are not on disk. A face scanned
    /// without them keeps its identity and is scored in place later.
    pub smile: Option<f64>,
    pub eyes_open: Option<f64>,
}

impl Found {
    /// The rectangle, as fractions, in the order the overlap arithmetic
    /// wants it.
    pub fn rectangle(&self) -> (f64, f64, f64, f64) {
        (self.x, self.y, self.width, self.height)
    }
}

/// The four networks, loaded.
pub struct Engine {
    detector: Net,
    /// Where each of YuNet's twelve heads is in the output list, resolved by
    /// name at load. Reading them by position would work until the day a
    /// re-exported model listed them in another order, and then it would
    /// find faces in the wrong places rather than fail.
    heads: [usize; 12],
    recognizer: Net,
    expressions: Option<expression::Nets>,
    directory: PathBuf,
}

impl std::fmt::Debug for Engine {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Engine")
            .field("directory", &self.directory)
            .field("expressions", &self.expressions.is_some())
            .finish()
    }
}

/// The twelve output names, in the order [`Engine::heads`] indexes them:
/// classification, objectness, boxes and landmarks, each for the three
/// grids.
const HEAD_NAMES: [&str; 12] = [
    "cls_8", "cls_16", "cls_32", //
    "obj_8", "obj_16", "obj_32", //
    "bbox_8", "bbox_16", "bbox_32", //
    "kps_8", "kps_16", "kps_32",
];

impl Engine {
    /// Loads what is in the folder.
    ///
    /// The two recognition models are required; the two expression models
    /// are not, and a damaged one costs only the expressions. A scan that
    /// refused to run because a smile could not be read would be the wrong
    /// trade in every direction.
    pub fn load(directory: &Path) -> Result<Self> {
        let detector =
            net(&directory.join(DETECTOR), (1, 3, CANVAS, CANVAS)).with_context(|| {
                format!(
                    "the face detector in {} cannot be read",
                    directory.display()
                )
            })?;
        let heads = resolve_heads(&detector)?;
        let recognizer = net(
            &directory.join(RECOGNIZER),
            (1, 3, align::ALIGNED, align::ALIGNED),
        )
        .with_context(|| {
            format!(
                "the face recogniser in {} cannot be read",
                directory.display()
            )
        })?;

        let expressions = match expression::Nets::load(directory) {
            Ok(nets) => Some(nets),
            Err(error) => {
                tracing::info!(
                    %error,
                    "scanning without expressions; the smile and eye models are missing or damaged"
                );
                None
            }
        };

        Ok(Self {
            detector,
            heads,
            recognizer,
            expressions,
            directory: directory.to_path_buf(),
        })
    }

    pub fn scores_expressions(&self) -> bool {
        self.expressions.is_some()
    }

    /// Every face in one photograph.
    ///
    /// `frame` is the photograph already decoded and turned the right way
    /// up — the same picture a person would see, because a detector handed a
    /// sideways frame finds nothing at all.
    pub fn find(&self, frame: &Rgb) -> Result<Vec<Found>> {
        if frame.width == 0 || frame.height == 0 {
            return Ok(Vec::new());
        }

        // The frame is letterboxed into the top-left of the square rather
        // than centred, which is what makes mapping a detection back a
        // single division. Centring would look tidier and buy nothing.
        let scale = (CANVAS as f64 / f64::from(frame.width))
            .min(CANVAS as f64 / f64::from(frame.height))
            .min(1.0);
        let canvas = letterbox(frame, scale);

        let outputs = self
            .detector
            .run(tvec!(canvas.into()))
            .context("the face detector failed")?;
        let tensors: Vec<&[f32]> = self
            .heads
            .iter()
            .map(|at| values(&outputs[*at]))
            .collect::<Result<Vec<_>>>()
            .context("the detector's output is not what this build expects")?;

        let grids: Vec<detect::Grid<'_>> = detect::STRIDES
            .iter()
            .enumerate()
            .map(|(index, stride)| detect::Grid {
                stride: *stride,
                classification: tensors[index],
                objectness: tensors[3 + index],
                boxes: tensors[6 + index],
                landmarks: tensors[9 + index],
            })
            .collect();

        let raw = detect::suppress(
            detect::decode(&grids, CANVAS, detect::SCORE_THRESHOLD),
            detect::NMS_THRESHOLD,
        );

        let minimum = CANVAS as f32 * MIN_FACE_FRACTION;
        let mut found = Vec::with_capacity(raw.len());
        for face in raw {
            if face.width < minimum || face.height < minimum {
                continue;
            }

            // Back onto the frame, where there is more of the face than the
            // canvas ever held.
            let back = |value: f32| f64::from(value) / scale;
            let mut landmarks = [(0f32, 0f32); 5];
            for (index, point) in face.landmarks.iter().enumerate() {
                landmarks[index] = ((back(point.0)) as f32, (back(point.1)) as f32);
            }

            let Some(aligned) = align::warp(frame, &landmarks) else {
                // Five landmarks that do not describe a face. Nothing to
                // recognise, and nothing worth reporting either.
                continue;
            };

            let embedding = self.embed(&aligned)?;
            let (smile, eyes_open) = match &self.expressions {
                Some(nets) => nets.score(&aligned),
                None => (None, None),
            };

            let width = f64::from(frame.width);
            let height = f64::from(frame.height);
            found.push(Found {
                x: (back(face.x) / width).clamp(0.0, 1.0),
                y: (back(face.y) / height).clamp(0.0, 1.0),
                width: (back(face.width) / width).clamp(0.0, 1.0),
                height: (back(face.height) / height).clamp(0.0, 1.0),
                confidence: f64::from(face.score),
                embedding,
                smile,
                eyes_open,
            });
        }

        Ok(found)
    }

    /// SFace over the aligned crop.
    ///
    /// The network takes the crop as it is: no scaling, no mean subtraction,
    /// channels in BGR because that is the order OpenCV had when the model
    /// was trained and the model has no opinion about which end is red.
    fn embed(&self, aligned: &[u8]) -> Result<Vec<f32>> {
        let tensor = planes(aligned, align::ALIGNED, align::ALIGNED, |value| value);
        let output = self
            .recognizer
            .run(tvec!(tensor.into()))
            .context("the face recogniser failed")?;
        let raw =
            values(&output[0]).context("the recogniser's output is not a vector of numbers")?;
        Ok(math::normalize(raw))
    }
}

/// The numbers behind a tensor.
///
/// tract hands results back as a `TValue`, which is a tensor of any type at
/// all; asking for the wrong one is an error rather than nonsense, which is
/// the point of going through here.
pub(crate) fn values(tensor: &TValue) -> Result<&[f32]> {
    tensor.view().as_slice::<f32>()
}

/// Loads one network and pins its input to a fixed shape.
fn net(path: &Path, shape: (usize, usize, usize, usize)) -> Result<Net> {
    anyhow::ensure!(path.is_file(), "{} is not there", path.display());
    let shape = [shape.0, shape.1, shape.2, shape.3];
    tract_onnx::onnx()
        .model_for_path(path)?
        .with_input_fact(0, f32::fact(shape).into())?
        .into_optimized()?
        .into_runnable()
}

/// Finds each of the twelve heads by name, and refuses a model that does not
/// have them.
fn resolve_heads(model: &Net) -> Result<[usize; 12]> {
    let outlets = model.model().output_outlets()?;
    let labels: Vec<Option<String>> = outlets
        .iter()
        .map(|outlet| {
            model
                .model()
                .outlet_label(*outlet)
                .map(str::to_owned)
                .or_else(|| Some(model.model().node(outlet.node).name.clone()))
        })
        .collect();

    let mut heads = [0usize; 12];
    for (slot, name) in HEAD_NAMES.iter().enumerate() {
        let at = labels
            .iter()
            .position(|label| label.as_deref() == Some(*name))
            .with_context(|| {
                format!(
                    "the detector has no output called {name}; \
                     this is not the YuNet model this build was written for"
                )
            })?;
        heads[slot] = at;
    }

    Ok(heads)
}

/// The frame scaled into the top-left of a black square, as the tensor the
/// detector wants.
fn letterbox(frame: &Rgb, scale: f64) -> Tensor {
    let width = ((f64::from(frame.width) * scale).round() as usize).clamp(1, CANVAS);
    let height = ((f64::from(frame.height) * scale).round() as usize).clamp(1, CANVAS);

    // Nearest-neighbour, and deliberately: this is the one place in the
    // application where a resample feeds a network rather than an eye. A
    // detector is unbothered by aliasing and a better resize would cost more
    // than the whole of the detection.
    let mut canvas = vec![0u8; CANVAS * CANVAS * 3];
    for y in 0..height {
        let source_y = (y as f64 / scale) as usize;
        let source_y = source_y.min(frame.height as usize - 1);
        for x in 0..width {
            let source_x = (x as f64 / scale) as usize;
            let source_x = source_x.min(frame.width as usize - 1);
            let from = (source_y * frame.width as usize + source_x) * 3;
            let to = (y * CANVAS + x) * 3;
            canvas[to..to + 3].copy_from_slice(&frame.pixels[from..from + 3]);
        }
    }

    planes(&canvas, CANVAS, CANVAS, |value| value)
}

/// RGB bytes as the NCHW float tensor every one of these networks wants,
/// with the channels reversed to BGR.
///
/// The reversal is not decoration. All four models were trained through
/// OpenCV, which reads a file into BGR and hands the network exactly that.
/// Feeding them RGB gives an answer of the same shape, which is why this is
/// worth a comment: it does not fail, it is quietly worse.
fn planes(rgb: &[u8], width: usize, height: usize, map: impl Fn(f32) -> f32) -> Tensor {
    let pixels = width * height;
    let mut data = vec![0f32; 3 * pixels];
    for (index, pixel) in rgb.as_chunks::<3>().0.iter().take(pixels).enumerate() {
        data[index] = map(f32::from(pixel[2]));
        data[pixels + index] = map(f32::from(pixel[1]));
        data[2 * pixels + index] = map(f32::from(pixel[0]));
    }

    tract_ndarray::Array4::from_shape_vec((1, 3, height, width), data)
        .expect("the plane buffer is built to this shape")
        .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn flat(width: u32, height: u32, colour: [u8; 3]) -> Rgb {
        Rgb::new(
            width,
            height,
            colour
                .iter()
                .cycle()
                .take((width * height * 3) as usize)
                .copied()
                .collect(),
        )
        .unwrap()
    }

    #[test]
    fn a_folder_with_nothing_in_it_says_which_models_are_missing() {
        let empty = tempfile::tempdir().unwrap();
        let availability = Availability::of(empty.path());
        assert!(!availability.recognition);
        assert!(!availability.expressions);
        assert_eq!(availability.missing.len(), 4);
        assert!(availability.missing.contains(&DETECTOR.to_owned()));
    }

    #[test]
    fn the_expression_models_are_optional_on_their_own() {
        let directory = tempfile::tempdir().unwrap();
        for name in [DETECTOR, RECOGNIZER] {
            std::fs::write(directory.path().join(name), b"not really a model").unwrap();
        }

        let availability = Availability::of(directory.path());
        assert!(availability.recognition);
        assert!(!availability.expressions);
        assert_eq!(availability.missing, vec![EMOTION, EYE_STATE]);
    }

    #[test]
    fn the_frame_lands_in_the_corner_of_the_canvas_at_its_own_ratio() {
        let frame = flat(1000, 500, [10, 20, 30]);
        let scale = (CANVAS as f64 / 1000.0).min(CANVAS as f64 / 500.0).min(1.0);
        let tensor = letterbox(&frame, scale);
        let values = tensor.view().as_slice::<f32>().unwrap();
        let pixels = CANVAS * CANVAS;

        // The first pixel is the photograph, in BGR.
        assert_eq!(values[0], 30.0);
        assert_eq!(values[pixels], 20.0);
        assert_eq!(values[2 * pixels], 10.0);

        // Below the letterboxed frame there is nothing but black. At this
        // scale the frame is 640x320, so row 400 is padding.
        let at = 400 * CANVAS;
        assert_eq!(values[at], 0.0);
    }

    #[test]
    fn a_frame_smaller_than_the_canvas_is_never_blown_up() {
        let frame = flat(100, 80, [255, 255, 255]);
        let scale = (CANVAS as f64 / 100.0).min(CANVAS as f64 / 80.0).min(1.0);
        assert_eq!(scale, 1.0);
        let tensor = letterbox(&frame, scale);
        let values = tensor.view().as_slice::<f32>().unwrap();
        // Column 100 of row 0 is outside the photograph.
        assert_eq!(values[100], 0.0);
        assert_eq!(values[99], 255.0);
    }

    /// Feeding a network RGB where it expects BGR does not fail — it gives a
    /// slightly wrong answer, forever. Hence a test rather than a comment
    /// alone.
    #[test]
    fn the_channels_reach_the_network_the_way_it_was_trained() {
        let tensor = planes(&[1, 2, 3], 1, 1, |value| value);
        assert_eq!(tensor.view().as_slice::<f32>().unwrap(), &[3.0, 2.0, 1.0]);
    }

    #[test]
    fn the_head_names_cover_all_three_grids() {
        for stride in detect::STRIDES {
            for prefix in ["cls", "obj", "bbox", "kps"] {
                let name = format!("{prefix}_{stride}");
                assert!(
                    HEAD_NAMES.contains(&name.as_str()),
                    "no output named {name}"
                );
            }
        }
    }
}
