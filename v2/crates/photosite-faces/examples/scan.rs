//! A folder of photographs through the face engine, printed.
//!
//! Not a test — it needs both the models and real photographs of real people,
//! and neither belongs in git. It is how the pipeline was checked against
//! reality, and how it can be checked again after a change to the decoding
//! or the alignment.
//!
//! ```text
//! cargo run --release -p photosite-faces --example scan -- <models> <folder> [count]
//! ```

fn main() -> anyhow::Result<()> {
    let mut args = std::env::args().skip(1);
    let models = args.next().expect("a models folder");
    let folder = args.next().expect("a folder of photographs");
    let count: usize = args.next().map(|n| n.parse().unwrap()).unwrap_or(10);

    let started = std::time::Instant::now();
    let engine = photosite_faces::Engine::load(std::path::Path::new(&models))?;
    println!(
        "engine loaded in {:?}, expressions: {}",
        started.elapsed(),
        engine.scores_expressions()
    );

    let mut photos: Vec<_> = std::fs::read_dir(&folder)?
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| {
            path.extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("jpg"))
        })
        .collect();
    photos.sort();
    photos.truncate(count);

    let mut total = 0;
    let whole = std::time::Instant::now();
    for path in &photos {
        let decoded = std::time::Instant::now();
        let frame = photosite_image::sized(path, 1024)?;
        let decoding = decoded.elapsed();
        let scanning = std::time::Instant::now();
        let faces = engine.find(&frame)?;
        total += faces.len();
        println!(
            "{:<28} {}x{}  decode {:>6.1?}  scan {:>6.1?}  {} face(s)",
            path.file_name().unwrap().to_string_lossy(),
            frame.width,
            frame.height,
            decoding,
            scanning.elapsed(),
            faces.len()
        );
        for face in &faces {
            println!(
                "    at {:.3},{:.3} {:.3}x{:.3}  confidence {:.3}  smile {}  eyes {}",
                face.x,
                face.y,
                face.width,
                face.height,
                face.confidence,
                face.smile
                    .map(|v| format!("{v:.2}"))
                    .unwrap_or_else(|| "-".into()),
                face.eyes_open
                    .map(|v| format!("{v:.2}"))
                    .unwrap_or_else(|| "-".into()),
            );
        }
    }

    println!(
        "\n{} photographs, {total} faces, {:?}",
        photos.len(),
        whole.elapsed()
    );
    Ok(())
}
