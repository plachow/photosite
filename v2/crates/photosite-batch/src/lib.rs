//! Running a batch that has already been planned.
//!
//! The deciding happened in [`photosite_core::batch`], where a test can
//! watch it and no disk is touched. What is left here is the doing: decode,
//! resize, sharpen, write, carry the metadata across. Every step of it is
//! per photograph and independent of every other, which is what makes the
//! whole thing one line of parallelism and no coordination at all.
//!
//! **One photograph failing must never end the run.** A file open in
//! something else, a card pulled out half way, a JPEG that turns out to be
//! a text file — each of those costs one output and is counted, and the
//! other hundred and forty-nine still get written. A batch that stops on the
//! first problem is a batch somebody has to babysit.

use anyhow::{Context, Result};
use photosite_core::batch::{Carry, Format, Plan, Preset, Step};
use photosite_core::{Cancel, Progress};
use photosite_image as img;
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};

/// How a finished run went.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Outcome {
    pub written: usize,
    /// Planned as skipped from the start — a name already taken, nowhere to
    /// put it.
    pub skipped: usize,
    pub failed: usize,
    /// What went wrong, one line per photograph, so somebody can be told
    /// which ones rather than how many.
    pub errors: Vec<String>,
    pub cancelled: bool,
}

/// Converts everything a plan says to convert.
///
/// The order of the outputs is the plan's; the order they are *done* in is
/// whatever the threads settle on, which is why the counts are gathered
/// rather than added up as it goes.
pub fn run(
    plan: &Plan,
    preset: &Preset,
    threads: usize,
    cancel: &Cancel,
    progress: &Progress,
) -> Outcome {
    let work: Vec<&Step> = plan
        .steps
        .iter()
        .filter(|step| step.skipped.is_none())
        .collect();
    let total = work.len() as u64;
    let done = AtomicUsize::new(0);

    let (send, take) = crossbeam_channel::unbounded::<Result<(), String>>();
    std::thread::scope(|scope| {
        let queue = crossbeam_channel::bounded::<&Step>(threads.max(1) * 2);
        for _ in 0..threads.max(1) {
            let take_work = queue.1.clone();
            let send = send.clone();
            let done = &done;
            scope.spawn(move || {
                for step in take_work {
                    if cancel.cancelled() {
                        return;
                    }

                    let outcome = convert(step, preset).map_err(|error| {
                        format!(
                            "{}: {error:#}",
                            step.from
                                .file_name()
                                .map(|name| name.to_string_lossy().into_owned())
                                .unwrap_or_default()
                        )
                    });

                    let at = done.fetch_add(1, Ordering::Relaxed) as u64 + 1;
                    progress.report(
                        at,
                        Some(total),
                        step.to
                            .file_name()
                            .map(|name| name.to_string_lossy().into_owned())
                            .unwrap_or_default(),
                    );
                    if send.send(outcome).is_err() {
                        return;
                    }
                }
            });
        }

        drop(send);
        for step in &work {
            if cancel.cancelled() || queue.0.send(step).is_err() {
                break;
            }
        }

        drop(queue.0);
    });

    let mut outcome = Outcome {
        skipped: plan.skips(),
        cancelled: cancel.cancelled(),
        ..Default::default()
    };
    for result in take {
        match result {
            Ok(()) => outcome.written += 1,
            Err(error) => {
                tracing::warn!(%error, "a photograph could not be converted");
                outcome.failed += 1;
                // Enough to say which, not so many that the window becomes a
                // log file. The rest are in the log, which is where a
                // hundred failures belong.
                if outcome.errors.len() < 20 {
                    outcome.errors.push(error);
                }
            }
        }
    }

    outcome
}

/// One photograph, end to end.
fn convert(step: &Step, preset: &Preset) -> Result<()> {
    // At its own size. Asking the decoder for a ceiling here would silently
    // cap an export at whatever number happened to be in it.
    let source = img::sized(&step.from, u32::MAX)
        .with_context(|| format!("cannot decode {}", step.from.display()))?;

    let mut image = match photosite_core::batch::measure(source.width, source.height, preset) {
        Some((width, height)) => img::encode::resize(&source, width, height)?,
        None => source,
    };

    // After the downscale, which is where the softness it makes up for comes
    // from. Sharpening first and then scaling throws the work away.
    if preset.sharpen > 0 {
        image = img::encode::sharpen(&image, preset.sharpen);
    }

    let format = output_format(&step.to, preset);
    img::encode::write(&step.to, &image, format, preset.quality)?;

    match preset.carry {
        Carry::Nothing => {}
        Carry::Everything => photosite_meta::carry(&step.from, &step.to, true)?,
        Carry::WithoutPlace => photosite_meta::carry(&step.from, &step.to, false)?,
    }

    Ok(())
}

/// What the output is written as.
///
/// The plan already put the right extension on the name, so the extension is
/// what decides — the preset's `Same` has no answer of its own, and one
/// place working it out means the name and the bytes cannot disagree.
fn output_format(destination: &Path, preset: &Preset) -> img::encode::Format {
    destination
        .extension()
        .and_then(|extension| extension.to_str())
        .and_then(img::encode::Format::of)
        .unwrap_or(match preset.format {
            Format::Png => img::encode::Format::Png,
            Format::WebP => img::encode::Format::WebP,
            Format::Tiff => img::encode::Format::Tiff,
            Format::Bmp => img::encode::Format::Bmp,
            _ => img::encode::Format::Jpeg,
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use photosite_core::batch::{OnCollision, Resize};
    use photosite_core::domain::{Organisation, Photo, PhotoId};
    use photosite_core::place::Verdict;
    use std::path::PathBuf;

    /// A real JPEG on disk, so the whole path is exercised rather than a
    /// mock of it.
    fn photograph(dir: &Path, name: &str, width: u32, height: u32) -> PathBuf {
        let mut pixels = Vec::with_capacity((width * height * 3) as usize);
        for y in 0..height {
            for x in 0..width {
                let value = ((x * 7 + y * 3) % 256) as u8;
                pixels.extend_from_slice(&[value, 255 - value, value / 2]);
            }
        }

        let image = img::Rgb::new(width, height, pixels).unwrap();
        let path = dir.join(name);
        img::encode::write(&path, &image, img::encode::Format::Jpeg, 92).unwrap();
        path
    }

    fn photo(path: &Path, width: u32, height: u32) -> Photo {
        Photo {
            id: PhotoId(1),
            path: path.to_path_buf(),
            folder: path.parent().unwrap().to_path_buf(),
            file_size: 0,
            modified_at: 0,
            taken_at: Some(1_655_300_000),
            width: Some(width),
            height: Some(height),
            orientation: 1,
            camera: None,
            lens: None,
            organisation: Organisation::default(),
            description_en: None,
            people: Vec::new(),
            expressions: Default::default(),
            place: None,
            verdict: Verdict::Nowhere,
            reason: None,
        }
    }

    fn go(photos: &[Photo], preset: &Preset) -> (Outcome, Plan) {
        let plan = photosite_core::batch::plan(photos, preset, &|path| path.exists());
        let tasks = photosite_core::Tasks::new();
        let outcome = tasks
            .here("batch", |cancel, progress| {
                Ok(run(&plan, preset, 2, cancel, progress))
            })
            .unwrap();
        (outcome, plan)
    }

    #[test]
    fn a_batch_writes_what_the_plan_said_it_would() {
        let dir = tempfile::tempdir().unwrap();
        let out = dir.path().join("out");
        let photos: Vec<Photo> = (0..4)
            .map(|n| {
                let path = photograph(dir.path(), &format!("{n}.jpg"), 60, 40);
                photo(&path, 60, 40)
            })
            .collect();

        let preset = Preset {
            into: Some(out.to_string_lossy().into_owned()),
            format: Format::Png,
            ..Default::default()
        };
        let (outcome, plan) = go(&photos, &preset);

        assert_eq!(outcome.written, 4, "{:?}", outcome.errors);
        assert_eq!(outcome.failed, 0);
        for step in &plan.steps {
            assert!(step.to.is_file(), "{} was not written", step.to.display());
        }
    }

    #[test]
    fn a_resize_reaches_the_file_on_disk() {
        let dir = tempfile::tempdir().unwrap();
        let path = photograph(dir.path(), "big.jpg", 400, 300);
        let preset = Preset {
            into: Some(dir.path().join("out").to_string_lossy().into_owned()),
            resize: Resize::LongestSide,
            resize_to: 100,
            ..Default::default()
        };
        let (outcome, plan) = go(&[photo(&path, 400, 300)], &preset);

        assert_eq!(outcome.written, 1, "{:?}", outcome.errors);
        let written = img::sized(&plan.steps[0].to, u32::MAX).unwrap();
        assert_eq!(
            (written.width, written.height),
            (100, 75),
            "the file is not the size the plan measured"
        );
    }

    /// A file open elsewhere, a JPEG that is not one — each costs its own
    /// output and nothing else.
    #[test]
    fn one_photograph_failing_does_not_end_the_run() {
        let dir = tempfile::tempdir().unwrap();
        let good = photograph(dir.path(), "good.jpg", 40, 30);
        let bad = dir.path().join("bad.jpg");
        std::fs::write(&bad, b"this is not a photograph").unwrap();

        let preset = Preset {
            into: Some(dir.path().join("out").to_string_lossy().into_owned()),
            ..Default::default()
        };
        let (outcome, _) = go(&[photo(&good, 40, 30), photo(&bad, 40, 30)], &preset);

        assert_eq!(outcome.written, 1);
        assert_eq!(outcome.failed, 1);
        assert_eq!(outcome.errors.len(), 1);
        assert!(
            outcome.errors[0].contains("bad.jpg"),
            "{:?}",
            outcome.errors
        );
    }

    #[test]
    fn a_skipped_photograph_is_counted_and_not_written() {
        let dir = tempfile::tempdir().unwrap();
        let path = photograph(dir.path(), "one.jpg", 40, 30);
        let out = dir.path().join("out");
        std::fs::create_dir_all(&out).unwrap();
        std::fs::write(out.join("one.jpg"), b"already here").unwrap();

        let preset = Preset {
            into: Some(out.to_string_lossy().into_owned()),
            on_collision: OnCollision::Skip,
            ..Default::default()
        };
        let (outcome, _) = go(&[photo(&path, 40, 30)], &preset);

        assert_eq!(outcome.written, 0);
        assert_eq!(outcome.skipped, 1);
        assert_eq!(
            std::fs::read(out.join("one.jpg")).unwrap(),
            b"already here",
            "the file that was there got written over"
        );
    }

    /// The one thing a batch must never do.
    #[test]
    fn the_photograph_being_read_is_still_there_afterwards() {
        let dir = tempfile::tempdir().unwrap();
        let path = photograph(dir.path(), "one.jpg", 40, 30);
        let before = std::fs::read(&path).unwrap();

        let preset = Preset {
            beside_source: true,
            format: Format::Same,
            on_collision: OnCollision::Overwrite,
            ..Default::default()
        };
        let (outcome, _) = go(&[photo(&path, 40, 30)], &preset);

        assert_eq!(outcome.written, 1, "{:?}", outcome.errors);
        assert_eq!(std::fs::read(&path).unwrap(), before, "the source changed");
    }

    #[test]
    fn what_the_photograph_said_about_itself_travels_with_it() {
        let dir = tempfile::tempdir().unwrap();
        let path = photograph(dir.path(), "one.jpg", 60, 40);
        photosite_meta::write(
            &path,
            &Organisation {
                rating: 4,
                title: Some("Sunrise".to_owned()),
                keywords: vec!["holiday".to_owned()],
                ..Default::default()
            },
            photosite_core::place::Place::new(50.0755, 14.4378),
            None,
        )
        .unwrap();

        let preset = Preset {
            into: Some(dir.path().join("out").to_string_lossy().into_owned()),
            carry: Carry::Everything,
            ..Default::default()
        };
        let (outcome, plan) = go(&[photo(&path, 60, 40)], &preset);
        assert_eq!(outcome.written, 1, "{:?}", outcome.errors);

        let said = photosite_meta::read(&plan.steps[0].to);
        assert_eq!(said.rating, Some(4));
        assert_eq!(said.title.as_deref(), Some("Sunrise"));
        assert_eq!(said.keywords, ["holiday"]);
        assert!(said.place.is_some(), "the position did not travel");
    }

    /// The reason somebody exports at all before putting a photograph of
    /// their house on the internet.
    #[test]
    fn a_position_can_be_left_behind_while_everything_else_travels() {
        let dir = tempfile::tempdir().unwrap();
        let path = photograph(dir.path(), "one.jpg", 60, 40);
        photosite_meta::write(
            &path,
            &Organisation {
                rating: 3,
                ..Default::default()
            },
            photosite_core::place::Place::new(50.0755, 14.4378),
            None,
        )
        .unwrap();

        let preset = Preset {
            into: Some(dir.path().join("out").to_string_lossy().into_owned()),
            carry: Carry::WithoutPlace,
            ..Default::default()
        };
        let (_, plan) = go(&[photo(&path, 60, 40)], &preset);

        let said = photosite_meta::read(&plan.steps[0].to);
        assert_eq!(said.rating, Some(3), "everything else should travel");
        assert_eq!(said.place, None, "the position travelled anyway");

        let raw = std::fs::read(&plan.steps[0].to).unwrap();
        assert!(
            photosite_image::exif::read(&raw).gps.is_none(),
            "the EXIF position travelled anyway"
        );
    }

    #[test]
    fn carrying_nothing_carries_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let path = photograph(dir.path(), "one.jpg", 60, 40);
        photosite_meta::write(
            &path,
            &Organisation {
                rating: 5,
                ..Default::default()
            },
            None,
            None,
        )
        .unwrap();

        let preset = Preset {
            into: Some(dir.path().join("out").to_string_lossy().into_owned()),
            carry: Carry::Nothing,
            ..Default::default()
        };
        let (_, plan) = go(&[photo(&path, 60, 40)], &preset);
        assert_eq!(photosite_meta::read(&plan.steps[0].to).rating, None);
    }

    /// No sidecars beside a conversion. A folder of exported files with a
    /// `.xmp` next to each one is not what anybody meant by "export".
    #[test]
    fn a_conversion_leaves_no_sidecars_lying_about() {
        let dir = tempfile::tempdir().unwrap();
        let path = photograph(dir.path(), "one.jpg", 40, 30);
        photosite_meta::write(
            &path,
            &Organisation {
                rating: 5,
                ..Default::default()
            },
            None,
            None,
        )
        .unwrap();

        let out = dir.path().join("out");
        for format in [Format::Png, Format::WebP, Format::Tiff, Format::Bmp] {
            let preset = Preset {
                into: Some(out.to_string_lossy().into_owned()),
                format,
                ..Default::default()
            };
            let (outcome, _) = go(&[photo(&path, 40, 30)], &preset);
            assert_eq!(outcome.written, 1, "{format:?}: {:?}", outcome.errors);
        }

        let sidecars: Vec<_> = std::fs::read_dir(&out)
            .unwrap()
            .flatten()
            .filter(|entry| entry.path().extension().is_some_and(|e| e == "xmp"))
            .collect();
        assert!(sidecars.is_empty(), "{sidecars:?}");
    }

    #[test]
    fn a_cancelled_run_says_so_and_stops() {
        let dir = tempfile::tempdir().unwrap();
        let photos: Vec<Photo> = (0..3)
            .map(|n| {
                let path = photograph(dir.path(), &format!("{n}.jpg"), 40, 30);
                photo(&path, 40, 30)
            })
            .collect();
        let preset = Preset {
            into: Some(dir.path().join("out").to_string_lossy().into_owned()),
            ..Default::default()
        };
        let plan = photosite_core::batch::plan(&photos, &preset, &|path| path.exists());
        let tasks = photosite_core::Tasks::new();
        let outcome = tasks
            .here("batch", |cancel, progress| {
                // Cancelled before it starts: nothing is written, and it
                // says why rather than looking like a run that found
                // nothing to do.
                cancel.stop();
                Ok(run(&plan, &preset, 2, cancel, progress))
            })
            .unwrap();

        assert!(outcome.cancelled);
        assert_eq!(outcome.written, 0);
    }
}
