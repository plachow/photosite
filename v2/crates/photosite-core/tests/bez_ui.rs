//! Hlídá hranici, která se rozpadá sama od sebe.
//!
//! Jádro ani obrázková crate nesmí mít v grafu závislostí grafickou knihovnu.
//! Není to čistota pro čistotu: přesně ztráta téhle hranice je jediný důvod,
//! proč byl port v1 drahý — devatenáct souborů mimo UI složky tam sahalo na
//! `BitmapSource` a nešly přeložit bez WPF.
//!
//! Test se ptá cargo, ne zdrojáků. Nepřímá závislost přes třetí crate by se
//! v `use` řádcích nepoznala.

use std::process::Command;

const ZAKAZANE: &[&str] = &["egui", "eframe", "wgpu", "winit", "epaint"];

fn zavislosti(crate_name: &str) -> Option<String> {
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

fn overit(crate_name: &str) {
    let Some(tree) = zavislosti(crate_name) else {
        // Bez cargo se test neprovede; v CI je vždycky.
        eprintln!("cargo tree nešlo spustit, hranice neověřena");
        return;
    };

    for zakazane in ZAKAZANE {
        let nalezeno = tree
            .lines()
            .map(str::trim)
            .any(|line| line.split_whitespace().next() == Some(zakazane));
        assert!(
            !nalezeno,
            "{crate_name} má v závislostech {zakazane}. \
             Tahle crate nesmí vědět, že existuje UI — přesně tady se ta hranice \
             ztrácí a s ní i možnost UI vyměnit."
        );
    }
}

#[test]
fn jadro_nezavisi_na_ui() {
    overit("photosite-core");
}

#[test]
fn obrazky_nezavisi_na_ui() {
    overit("photosite-image");
}
