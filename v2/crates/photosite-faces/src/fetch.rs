//! Where the four models come from, and fetching them when asked.
//!
//! The crate's own documentation says nothing leaves the computer, and
//! this is the one place it speaks to the network at all: to bring the
//! models *in*, once, from where their authors publish them, and only when
//! somebody presses the button. A fresh install has none of them — they are
//! not shipped, for the reasons `packaging/README.md` gives — and "download
//! them from the OpenCV Zoo, the ONNX Model Zoo and the OpenVINO storage
//! under exactly these names" is not an instruction anybody follows.
//! Nobody reads a README; the window has to offer.
//!
//! Every address is pinned to a commit or a release and every file to a
//! SHA-256, because a model is executable in every way that matters: a
//! different file under the same name would find faces in the wrong
//! places, or nowhere, and the sweep would have no way to say so. A
//! download that does not hash to what was expected is thrown away, with
//! its reason.
//!
//! Each file is written beside its final name with a `.part` suffix and
//! moved over it only once the hash is right, so a download stopped halfway
//! — by the person, or by the network — leaves nothing that
//! [`Engine::load`](crate::Engine::load) could mistake for a model.

use anyhow::{Context, Result, ensure};
use photosite_core::{Cancel, Progress};
use sha2::{Digest, Sha256};
use std::io::{Read, Write};
use std::path::Path;
use std::time::Duration;

/// One of the four, and where it is published.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Model {
    /// The file name, as [`Availability`](crate::Availability) looks for it.
    pub file: &'static str,
    /// What it is called in conversation.
    pub name: &'static str,
    /// Who publishes it.
    pub source: &'static str,
    pub licence: &'static str,
    /// Pinned to a commit or a release, never to a branch.
    pub url: &'static str,
    /// Lower-case hex, of the file as published.
    pub sha256: &'static str,
    /// What it weighs, so the offer can say so before anything is fetched.
    pub bytes: u64,
    /// Whether a sweep can run without it.
    pub required: bool,
}

/// The four, in the order a scan needs them.
pub static MODELS: [Model; 4] = [
    Model {
        file: crate::DETECTOR,
        name: "YuNet",
        source: "OpenCV Zoo",
        licence: "Apache-2.0",
        url: "https://github.com/opencv/opencv_zoo/raw/f12e12798e8314f7c074a6656816c048dcc95b7a/models/face_detection_yunet/face_detection_yunet_2023mar.onnx",
        sha256: "8f2383e4dd3cfbb4553ea8718107fc0423210dc964f9f4280604804ed2552fa4",
        bytes: 232_589,
        required: true,
    },
    Model {
        file: crate::RECOGNIZER,
        name: "SFace",
        source: "OpenCV Zoo",
        licence: "Apache-2.0",
        url: "https://github.com/opencv/opencv_zoo/raw/ba91a3b91d00d76e86540d4013f944bd6b514e39/models/face_recognition_sface/face_recognition_sface_2021dec.onnx",
        sha256: "0ba9fbfa01b5270c96627c4ef784da859931e02f04419c829e83484087c34e79",
        bytes: 38_696_353,
        required: true,
    },
    Model {
        file: crate::EMOTION,
        name: "FER+",
        source: "ONNX Model Zoo",
        licence: "MIT",
        url: "https://github.com/onnx/models/raw/4c46cd00fbdb7cd30b6c1c17ab54f2e1f4f7b177/validated/vision/body_analysis/emotion_ferplus/model/emotion-ferplus-8.onnx",
        sha256: "a2a2ba6a335a3b29c21acb6272f962bd3d47f84952aaffa03b60986e04efa61c",
        bytes: 35_040_571,
        required: false,
    },
    Model {
        file: crate::EYE_STATE,
        name: "open-closed-eye-0001",
        source: "OpenVINO Open Model Zoo",
        licence: "Apache-2.0",
        url: "https://storage.openvinotoolkit.org/repositories/open_model_zoo/public/2022.1/open-closed-eye-0001/open_closed_eye.onnx",
        sha256: "4daa100034482525a26c9afb9297c16580a531189e66e3d2b2ac7d32becfd593",
        bytes: 46_164,
        required: false,
    },
];

/// How much arrives between two reports to the task board. Every chunk
/// would be a lock per 64 KiB for a progress bar nobody can see move that
/// finely.
const REPORT_EVERY: u64 = 1 << 20;

/// The ones not on disk, in the order they are fetched.
pub fn missing(directory: &Path) -> Vec<&'static Model> {
    MODELS
        .iter()
        .filter(|model| !directory.join(model.file).is_file())
        .collect()
}

/// What a set of them weighs together.
pub fn bytes_of(models: &[&Model]) -> u64 {
    models.iter().map(|model| model.bytes).sum()
}

/// A size as somebody would say it: "74" for 74 MB, or "0.2" for a model
/// small enough that a whole number would read as nothing.
pub fn megabytes(bytes: u64) -> String {
    let megabytes = bytes as f64 / 1_000_000.0;
    if megabytes >= 10.0 {
        format!("{}", megabytes.round() as u64)
    } else {
        format!("{megabytes:.1}")
    }
}

/// Fetches `models` into `directory`, one after another, reporting bytes
/// over the whole set.
///
/// `label` names the model being fetched in the words the caller wants on
/// the board; this crate holds no translations. Stopped through `cancel`,
/// it returns `Ok` with whatever was finished already in place and the
/// half-fetched one gone — the task board marks it cancelled, and the
/// caller looks at the folder again rather than at the result.
pub fn fetch(
    directory: &Path,
    models: &[&'static Model],
    cancel: &Cancel,
    progress: &Progress,
    label: impl Fn(&Model) -> String,
) -> Result<()> {
    std::fs::create_dir_all(directory)
        .with_context(|| format!("{} cannot be created", directory.display()))?;

    let total = bytes_of(models);
    let agent = agent();
    let mut before = 0;
    for &model in models {
        if cancel.cancelled() {
            return Ok(());
        }

        let label = label(model);
        progress.report(before, Some(total), label.as_str());
        tracing::info!(file = model.file, url = model.url, "fetching a face model");
        if !download(
            &agent, model, directory, cancel, progress, before, total, &label,
        )? {
            tracing::info!(file = model.file, "stopped; the part file is gone");
            return Ok(());
        }

        before += model.bytes;
        progress.report(before, Some(total), label.as_str());
    }

    Ok(())
}

/// Connects, and judges certificates by the operating system's own store —
/// the same reason `photosite-ai` gives: a machine behind a re-signing
/// proxy trusts it there and nowhere else.
///
/// There is a timeout on getting an answer and none on the body: forty
/// megabytes over a slow line is minutes, and a figure that is generous
/// for one connection is a failure on another. Stopping is the person's
/// to do, through the task.
fn agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_resolve(Some(Duration::from_secs(30)))
        .timeout_connect(Some(Duration::from_secs(30)))
        .timeout_recv_response(Some(Duration::from_secs(60)))
        .http_status_as_error(false)
        .tls_config(
            ureq::tls::TlsConfig::builder()
                .root_certs(ureq::tls::RootCerts::PlatformVerifier)
                .build(),
        )
        .build()
        .into()
}

/// One model: to a part file, checked, then into place. `Ok(false)` is a
/// stop; either way nothing but a finished, verified model is ever left
/// under its final name.
#[allow(clippy::too_many_arguments)]
fn download(
    agent: &ureq::Agent,
    model: &Model,
    directory: &Path,
    cancel: &Cancel,
    progress: &Progress,
    before: u64,
    total: u64,
    label: &str,
) -> Result<bool> {
    let destination = directory.join(model.file);
    let part = directory.join(format!("{}.part", model.file));
    let outcome = pull(agent, model, &part, cancel, progress, before, total, label);
    match outcome {
        Ok(true) => {
            std::fs::rename(&part, &destination).with_context(|| {
                format!(
                    "{} cannot be moved into place as {}",
                    part.display(),
                    destination.display()
                )
            })?;
            Ok(true)
        }
        outcome => {
            // Whatever is in it is not a model.
            let _ = std::fs::remove_file(&part);
            outcome
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn pull(
    agent: &ureq::Agent,
    model: &Model,
    part: &Path,
    cancel: &Cancel,
    progress: &Progress,
    before: u64,
    total: u64,
    label: &str,
) -> Result<bool> {
    let mut response = agent
        .get(model.url)
        .call()
        .with_context(|| format!("{} could not be fetched from {}", model.name, model.url))?;
    let status = response.status().as_u16();
    ensure!(
        (200..300).contains(&status),
        "{} could not be fetched: HTTP {status} from {}",
        model.name,
        model.url
    );

    let mut body = response.body_mut().as_reader();
    let mut file = std::fs::File::create(part)
        .with_context(|| format!("{} cannot be written", part.display()))?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0u8; 64 * 1024];
    let mut received: u64 = 0;
    let mut reported: u64 = 0;
    loop {
        if cancel.cancelled() {
            return Ok(false);
        }

        let count = body
            .read(&mut buffer)
            .with_context(|| format!("{} stopped arriving from {}", model.name, model.url))?;
        if count == 0 {
            break;
        }

        file.write_all(&buffer[..count])
            .with_context(|| format!("{} cannot be written", part.display()))?;
        hasher.update(&buffer[..count]);
        received += count as u64;
        // A file that keeps coming past its published size is not the
        // file; stop reading rather than fill the disk finding out.
        ensure!(
            received <= model.bytes,
            "{} is larger than the {} bytes its authors published",
            model.name,
            model.bytes
        );

        if received - reported >= REPORT_EVERY {
            reported = received;
            progress.report(before + received, Some(total), label);
        }
    }

    file.flush()
        .with_context(|| format!("{} cannot be written", part.display()))?;
    // Closed before it is renamed: Windows will not move an open file.
    drop(file);

    ensure!(
        received == model.bytes,
        "{} arrived short: {received} of {} bytes",
        model.name,
        model.bytes
    );
    let digest = hex(&hasher.finalize());
    ensure!(
        digest == model.sha256,
        "{} is not the file its authors published: SHA-256 {digest}, expected {}",
        model.name,
        model.sha256
    );

    Ok(true)
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Availability;

    #[test]
    fn the_four_are_the_four_availability_looks_for() {
        let files: Vec<&str> = MODELS.iter().map(|model| model.file).collect();
        assert_eq!(
            files,
            [
                crate::DETECTOR,
                crate::RECOGNIZER,
                crate::EMOTION,
                crate::EYE_STATE
            ]
        );
    }

    /// A branch name in an address is a file that changes under the hash.
    #[test]
    fn every_address_is_pinned_and_hashed() {
        for model in &MODELS {
            assert!(model.url.starts_with("https://"), "{}", model.url);
            assert!(
                !model.url.contains("/main/") && !model.url.contains("/master/"),
                "{} is not pinned",
                model.url
            );
            assert_eq!(model.sha256.len(), 64, "{}", model.name);
            assert!(
                model
                    .sha256
                    .chars()
                    .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()),
                "{}",
                model.name
            );
            assert!(model.bytes > 0, "{}", model.name);
        }
    }

    #[test]
    fn an_empty_folder_wants_all_four_and_a_full_one_none() {
        let directory = tempfile::tempdir().unwrap();
        let wanted: Vec<&str> = missing(directory.path())
            .into_iter()
            .map(|model| model.file)
            .collect();
        assert_eq!(wanted.len(), 4);
        assert_eq!(wanted, Availability::of(directory.path()).missing);

        for model in &MODELS {
            std::fs::write(directory.path().join(model.file), b"x").unwrap();
        }

        assert!(missing(directory.path()).is_empty());
    }

    /// What is required here is what `Availability` requires there: the
    /// two the sweep cannot run without, and no more.
    #[test]
    fn the_required_two_are_the_recognition_two() {
        let directory = tempfile::tempdir().unwrap();
        for model in MODELS.iter().filter(|model| model.required) {
            std::fs::write(directory.path().join(model.file), b"x").unwrap();
        }

        let availability = Availability::of(directory.path());
        assert!(availability.recognition);
        assert!(!availability.expressions);
    }

    #[test]
    fn sizes_read_as_somebody_would_say_them() {
        assert_eq!(megabytes(74_015_677), "74");
        assert_eq!(megabytes(232_589), "0.2");
        assert_eq!(megabytes(46_164), "0.0");
        let all: Vec<&Model> = MODELS.iter().collect();
        assert_eq!(megabytes(bytes_of(&all)), "74");
    }

    #[test]
    fn hex_is_lower_case_and_padded() {
        assert_eq!(hex(&[0, 15, 255]), "000fff");
    }
}
