//! Sweeping a folder for faces, and writing what was found.
//!
//! This is the one part of the face work that needs both halves — the
//! networks and the catalogue — and it is here rather than in the window so
//! that it can be run without one. The CLI runs the same function, which is
//! how a sweep gets measured on a real library without a screen or a GPU.
//!
//! **Decoding and inference happen on every core; the catalogue is written
//! on one.** There is a single catalogue and writing it twice at once is the
//! one thing that cannot be done, so the workers hand back observations and
//! the caller's thread writes them. That is also what keeps the counts
//! honest: they are added up in one place.

use crate::{Engine, cluster, detect, math};
use anyhow::Result;
use photosite_core::people::Observation;
use photosite_core::{Cancel, Catalog, FileIdentity, Photo, Progress};
use std::path::Path;
use std::sync::Arc;

/// How far two rectangles must agree before one is taken to be the other.
///
/// Used only by the scoring pass, to recognise a face it has already stored.
/// Half is generous — the two detections come from the same model on the
/// same photograph and differ only by the decode size — and being generous
/// is right: the alternative to matching is leaving a face unscored, which
/// costs nothing but another pass.
const SAME_FACE: f64 = 0.5;

/// What one sweep did.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Report {
    pub photographs: u64,
    pub faces: u64,
    /// Recognised as somebody already named, and the name written into the
    /// photograph without anybody watching.
    pub assigned: u64,
    /// Probably somebody, waiting for a yes or a no.
    pub suggested: u64,
    /// Faces the expression models had never seen and now have.
    pub scored: u64,
    /// Files that could not be read at all. A sweep of ten thousand must
    /// never end because one of them is half-written.
    pub failed: u64,
}

/// The sweep proper.
///
/// Decoding and inference happen on every core; the catalogue is written on
/// this one. That split is the whole shape of it — there is one catalogue
/// and writing it twice at once is the one thing that cannot be done — and
/// it is why the workers hand back observations rather than writing them.
#[allow(clippy::too_many_arguments)]
pub fn sweep(
    catalog: &mut Catalog,
    engine: &Arc<Engine>,
    folder: &Path,
    recursive: bool,
    detect: u32,
    threads: usize,
    now: i64,
    cancel: &Cancel,
    progress: &Progress,
) -> Result<Report> {
    let photos = catalog.unscanned_faces(folder, recursive)?;
    let total = photos.len() as u64;
    let mut report = Report::default();
    if photos.is_empty() {
        return Ok(report);
    }

    // Who is already known, as one vector each. Read once: a face assigned
    // during this very sweep does not move the average until the next one,
    // which is what keeps a long sweep from drifting away from what
    // somebody actually named.
    let known: Arc<Vec<cluster::Known>> = Arc::new(
        catalog
            .person_embeddings()?
            .into_iter()
            .map(|(id, embeddings)| cluster::Known {
                id,
                centroid: math::centroid(&embeddings),
            })
            .collect(),
    );

    let (send_work, take_work) = crossbeam_channel::bounded::<Photo>(threads * 2);
    let (send_done, take_done) =
        crossbeam_channel::bounded::<(Photo, Option<Vec<Observation>>)>(threads * 2);

    std::thread::scope(|scope| -> Result<()> {
        for _ in 0..threads.max(1) {
            let take_work = take_work.clone();
            let send_done = send_done.clone();
            let engine = engine.clone();
            let known = known.clone();
            scope.spawn(move || {
                for photo in take_work {
                    let found = look(&engine, &photo.path, detect, &known);
                    if send_done.send((photo, found)).is_err() {
                        return;
                    }
                }
            });
        }

        drop(take_work);
        drop(send_done);

        let mut queue = photos.into_iter();
        let mut in_flight = 0usize;
        let mut sender = Some(send_work);
        loop {
            // Keep the workers fed, but never more than the channel holds:
            // handing them the whole folder would mean a cancelled sweep
            // still had thousands of photographs to decode before it
            // noticed.
            while in_flight < threads * 2 && !cancel.cancelled() {
                match queue.next() {
                    Some(photo) => match sender.as_ref() {
                        Some(send) if send.send(photo).is_ok() => in_flight += 1,
                        _ => break,
                    },
                    None => {
                        sender = None;
                        break;
                    }
                }
            }

            if cancel.cancelled() {
                sender = None;
            }

            if in_flight == 0 {
                break;
            }

            let Ok((photo, found)) = take_done.recv() else {
                break;
            };
            in_flight -= 1;

            let Some(found) = found else {
                report.failed += 1;
                continue;
            };

            let mut names: Vec<String> = Vec::new();
            for face in &found {
                match face.person {
                    Some(person) => {
                        report.assigned += 1;
                        if let Some(name) = name_of(catalog, person)
                            && !names.contains(&name)
                        {
                            names.push(name);
                        }
                    }
                    None if face.suggested.is_some() => report.suggested += 1,
                    None => {}
                }
            }

            report.faces += found.len() as u64;
            let identity = FileIdentity {
                path: photo.path.clone(),
                file_size: photo.file_size,
                modified_at: photo.modified_at,
            };
            catalog.replace_faces(photo.id, &identity, &found, now)?;

            // A face recognised without anybody watching still writes a name
            // into the photograph — the same name a person would have typed,
            // through the same path.
            if !names.is_empty() {
                catalog.add_keywords(&[photo.id], &names)?;

                // Only then. A first sweep of a library finds faces on
                // most of it and names on hardly any, and queueing every
                // photograph would rewrite the whole library to say
                // "nobody here is named" — thousands of files touched to
                // record an absence. Taking a name *off* a photograph
                // queues it explicitly, which is the case that has
                // something to unsay.
                catalog.enqueue(&[photo.id], now)?;
            }

            report.photographs += 1;
            progress.report(
                report.photographs,
                Some(total),
                photo
                    .path
                    .file_name()
                    .map(|name| name.to_string_lossy().into_owned())
                    .unwrap_or_default(),
            );
        }

        Ok(())
    })?;

    Ok(report)
}

/// One photograph through the engine, or nothing when it cannot be read.
///
/// An unreadable or vanished file must never end a sweep of ten thousand.
fn look(
    engine: &Engine,
    path: &Path,
    detect: u32,
    known: &[cluster::Known],
) -> Option<Vec<Observation>> {
    let frame = match photosite_image::sized(path, detect) {
        Ok(frame) => frame,
        Err(error) => {
            tracing::debug!(path = %path.display(), error = %format!("{error:#}"), "cannot be read for a face sweep");
            return None;
        }
    };

    let found = match engine.find(&frame) {
        Ok(found) => found,
        Err(error) => {
            tracing::warn!(path = %path.display(), error = %format!("{error:#}"), "the face sweep failed");
            return None;
        }
    };

    Some(
        found
            .into_iter()
            .map(|face| {
                let (verdict, _) = cluster::identify(&face.embedding, known);
                let (person, suggested) = match verdict {
                    cluster::Verdict::Assign(person) => (Some(person), None),
                    cluster::Verdict::Suggest(person) => (None, Some(person)),
                    cluster::Verdict::Unknown => (None, None),
                };
                Observation {
                    x: face.x,
                    y: face.y,
                    width: face.width,
                    height: face.height,
                    confidence: face.confidence,
                    embedding: face.embedding,
                    person,
                    suggested,
                    smile: face.smile,
                    eyes_open: face.eyes_open,
                }
            })
            .collect(),
    )
}

fn name_of(catalog: &Catalog, person: i64) -> Option<String> {
    catalog
        .people()
        .ok()?
        .into_iter()
        .find(|had| had.id == person)
        .map(|had| had.name)
}

/// Scores faces the expression models have never seen.
///
/// The photograph is looked at again and the scores are matched onto the
/// faces already stored **by overlap**, so every face keeps its number, its
/// person and its suggestion. Replacing them instead would forget everybody
/// on the photograph and find them again as strangers — which is exactly
/// what makes this a pass of its own rather than a rescan.
#[allow(clippy::too_many_arguments)]
pub fn score(
    catalog: &mut Catalog,
    engine: &Arc<Engine>,
    folder: &Path,
    recursive: bool,
    detect: u32,
    threads: usize,
    scoring: &str,
    cancel: &Cancel,
    progress: &Progress,
) -> Result<u64> {
    let photos = catalog.missing_expressions(folder, recursive)?;
    if photos.is_empty() {
        return Ok(0);
    }

    let total = photos.len() as u64;
    let mut done = 0u64;
    let mut scored = 0u64;
    let (send_work, take_work) = crossbeam_channel::bounded::<Photo>(threads * 2);
    let (send_done, take_done) =
        crossbeam_channel::bounded::<(Photo, Vec<crate::Found>)>(threads * 2);

    std::thread::scope(|scope| -> Result<()> {
        for _ in 0..threads.max(1) {
            let take_work = take_work.clone();
            let send_done = send_done.clone();
            let engine = engine.clone();
            scope.spawn(move || {
                for photo in take_work {
                    let found = photosite_image::sized(&photo.path, detect)
                        .ok()
                        .and_then(|frame| engine.find(&frame).ok())
                        .unwrap_or_default();
                    if send_done.send((photo, found)).is_err() {
                        return;
                    }
                }
            });
        }

        drop(take_work);
        drop(send_done);

        let mut queue = photos.into_iter();
        let mut in_flight = 0usize;
        let mut sender = Some(send_work);
        loop {
            while in_flight < threads * 2 && !cancel.cancelled() {
                match queue.next() {
                    Some(photo) => match sender.as_ref() {
                        Some(send) if send.send(photo).is_ok() => in_flight += 1,
                        _ => break,
                    },
                    None => {
                        sender = None;
                        break;
                    }
                }
            }

            if cancel.cancelled() {
                sender = None;
            }

            if in_flight == 0 {
                break;
            }

            let Ok((photo, found)) = take_done.recv() else {
                break;
            };
            in_flight -= 1;

            let stored = catalog.faces_of(photo.id)?;
            let mut scores: Vec<(i64, Option<f64>, Option<f64>)> = Vec::new();
            for face in stored
                .iter()
                .filter(|face| face.smile.is_none() || face.eyes_open.is_none())
            {
                let best = found
                    .iter()
                    .map(|candidate| {
                        (
                            candidate,
                            detect::overlap_of(face.rectangle(), candidate.rectangle()),
                        )
                    })
                    .filter(|(_, overlap)| *overlap >= SAME_FACE)
                    .max_by(|a, b| a.1.total_cmp(&b.1));
                if let Some((candidate, _)) = best
                    && (candidate.smile.is_some() || candidate.eyes_open.is_some())
                {
                    scores.push((face.id, candidate.smile, candidate.eyes_open));
                }
            }

            scored += scores.len() as u64;
            catalog.score_expressions(&scores)?;
            done += 1;
            progress.report(done, Some(total), scoring);
        }

        Ok(())
    })?;

    Ok(scored)
}
