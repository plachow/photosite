//! Guards the boundary that erodes on its own.
//!
//! Neither the core nor the image crate may carry a graphics library anywhere
//! in its dependency graph. This is not tidiness for its own sake: losing this
//! exact boundary is the single reason the v1 port was expensive — nineteen
//! files outside the UI folders reached for `BitmapSource` and would not
//! compile without WPF.
//!
//! The test asks cargo, not the sources. An indirect dependency through some
//! third crate would never show up in a `use` line.

use std::process::Command;

const FORBIDDEN: &[&str] = &["egui", "eframe", "wgpu", "winit", "epaint"];

fn dependencies(crate_name: &str) -> Option<String> {
    let manifest = concat!(env!("CARGO_MANIFEST_DIR"), "/../../Cargo.toml");
    let output = Command::new(option_env!("CARGO").unwrap_or("cargo"))
        .args([
            "tree",
            "--manifest-path",
            manifest,
            "-p",
            crate_name,
            "--prefix",
            "none",
            "--edges",
            "normal",
        ])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }

    Some(String::from_utf8_lossy(&output.stdout).into_owned())
}

fn check(crate_name: &str) {
    let Some(tree) = dependencies(crate_name) else {
        // Without cargo the test cannot run; on CI it is always there.
        eprintln!("could not run cargo tree, boundary not verified");
        return;
    };

    for forbidden in FORBIDDEN {
        let found = tree
            .lines()
            .map(str::trim)
            .any(|line| line.split_whitespace().next() == Some(forbidden));
        assert!(
            !found,
            "{crate_name} depends on {forbidden}. \
             This crate must not know a UI exists — this is exactly where that \
             boundary goes, and with it the option of replacing the UI."
        );
    }
}

#[test]
fn the_core_does_not_depend_on_the_ui() {
    check("photosite-core");
}

#[test]
fn images_do_not_depend_on_the_ui() {
    check("photosite-image");
}
