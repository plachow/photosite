//! What can only be checked with the real models on disk.
//!
//! The models are a hundred megabytes of downloaded binaries and are not in
//! git, so this test **skips itself** when they are not there rather than
//! failing. A test that fails on a fresh clone teaches everybody to ignore a
//! red run, which costs more than the test is worth.
//!
//! Point it at them with `PHOTOSITE_MODELS`, or leave them where the
//! application keeps them and they are found.

use photosite_faces::{Availability, Engine};
use photosite_image::Rgb;
use std::path::PathBuf;

/// Where the models are, if anywhere.
fn models() -> Option<PathBuf> {
    let directory = std::env::var_os("PHOTOSITE_MODELS").map(PathBuf::from)?;
    Availability::of(&directory)
        .recognition
        .then_some(directory)
}

fn flat(width: u32, height: u32, colour: [u8; 3]) -> Rgb {
    let pixels = colour
        .iter()
        .cycle()
        .take((width * height * 3) as usize)
        .copied()
        .collect();
    Rgb::new(width, height, pixels).unwrap()
}

/// The one thing that cannot be checked without the file: that this really
/// is the model this code was written against, with all twelve heads under
/// the names it looks them up by.
#[test]
fn the_models_load_and_are_the_ones_this_build_expects() {
    let Some(directory) = models() else {
        eprintln!("no models on disk; skipping");
        return;
    };

    let engine = Engine::load(&directory).expect("the models should load");
    assert_eq!(
        engine.scores_expressions(),
        Availability::of(&directory).expressions
    );
}

/// A photograph of nothing has no faces in it. Obvious, and the reason to
/// check it is that a wrong prior grid or an off-by-one in the decoding
/// produces detections out of flat grey.
#[test]
fn there_are_no_faces_in_a_blank_frame() {
    let Some(directory) = models() else {
        eprintln!("no models on disk; skipping");
        return;
    };

    let engine = Engine::load(&directory).expect("the models should load");
    for colour in [[0, 0, 0], [128, 128, 128], [255, 255, 255]] {
        let found = engine
            .find(&flat(1024, 683, colour))
            .expect("the scan should run");
        assert!(
            found.is_empty(),
            "{colour:?} produced {} faces",
            found.len()
        );
    }
}

/// A frame of no size at all is a real case — a file that decoded to
/// nothing — and it must come back empty rather than divide by a zero
/// dimension.
#[test]
fn a_frame_with_no_pixels_is_not_a_crash() {
    let Some(directory) = models() else {
        eprintln!("no models on disk; skipping");
        return;
    };

    let engine = Engine::load(&directory).expect("the models should load");
    let empty = Rgb::new(0, 0, Vec::new()).unwrap();
    assert!(engine.find(&empty).unwrap().is_empty());
}
