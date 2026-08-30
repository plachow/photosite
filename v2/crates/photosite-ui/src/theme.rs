//! Motiv a kreslení dlaždice.
//!
//! Barvy jsou v jedné struktuře a berou si je odsud všichni — mřížka, doky,
//! dialogy, posuvníky i combo. Přepnutí motivu je pak výměna jednoho ukazatele
//! mezi snímky, ne převazování stylů. O to jde na tom slově „konzistentní".

use egui::{Color32, CornerRadius, FontId, Rect, Stroke, StrokeKind, Vec2};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Palette {
    /// Klíč do nastavení. Název se smí přepsat i přeložit, tenhle ne.
    pub id: &'static str,
    pub title: &'static str,
    pub window: Color32,
    pub panel: Color32,
    /// Rám diapozitivu.
    pub tile: Color32,
    /// Plocha pod fotkou. Tmavší než rám, aby fotka „seděla v okně".
    pub well: Color32,
    /// Proužek s názvem.
    pub caption: Color32,
    pub text: Color32,
    pub dim: Color32,
    pub accent: Color32,
    /// Horní a levá hrana rámu. Jen náznak, ne vypouklé tlačítko.
    pub bevel_light: Color32,
    pub bevel_dark: Color32,
}

pub const PALETTES: [Palette; 3] = [
    Palette {
        id: "tmava",
        title: "Tmavě šedá",
        window: Color32::from_rgb(0x22, 0x22, 0x24),
        panel: Color32::from_rgb(0x2A, 0x2A, 0x2C),
        tile: Color32::from_rgb(0x3A, 0x3A, 0x3D),
        well: Color32::from_rgb(0x1A, 0x1A, 0x1C),
        caption: Color32::from_rgb(0x44, 0x44, 0x48),
        text: Color32::from_rgb(0xDA, 0xDA, 0xDE),
        dim: Color32::from_rgb(0x8A, 0x8A, 0x90),
        accent: Color32::from_rgb(0x5B, 0x9D, 0xD9),
        bevel_light: Color32::from_rgb(0x50, 0x50, 0x55),
        bevel_dark: Color32::from_rgb(0x16, 0x16, 0x18),
    },
    Palette {
        id: "svetla",
        title: "Světle šedá",
        window: Color32::from_rgb(0x3C, 0x3C, 0x3E),
        panel: Color32::from_rgb(0x46, 0x46, 0x48),
        tile: Color32::from_rgb(0x58, 0x58, 0x5B),
        well: Color32::from_rgb(0x2E, 0x2E, 0x30),
        caption: Color32::from_rgb(0x62, 0x62, 0x66),
        text: Color32::from_rgb(0xEC, 0xEC, 0xEE),
        dim: Color32::from_rgb(0xA6, 0xA6, 0xAA),
        accent: Color32::from_rgb(0x6F, 0xB0, 0xE8),
        bevel_light: Color32::from_rgb(0x74, 0x74, 0x78),
        bevel_dark: Color32::from_rgb(0x2A, 0x2A, 0x2C),
    },
    Palette {
        id: "sepie",
        title: "Sépie",
        window: Color32::from_rgb(0x26, 0x21, 0x1B),
        panel: Color32::from_rgb(0x2E, 0x28, 0x21),
        tile: Color32::from_rgb(0x40, 0x38, 0x2D),
        well: Color32::from_rgb(0x1B, 0x17, 0x12),
        caption: Color32::from_rgb(0x4C, 0x42, 0x35),
        text: Color32::from_rgb(0xE4, 0xD8, 0xC4),
        dim: Color32::from_rgb(0x99, 0x8C, 0x77),
        accent: Color32::from_rgb(0xC9, 0x94, 0x4F),
        bevel_light: Color32::from_rgb(0x58, 0x4D, 0x3E),
        bevel_dark: Color32::from_rgb(0x18, 0x14, 0x10),
    },
];

/// Najde paletu podle klíče z nastavení. Neznámý klíč spadne na první —
/// překlep v konfiguraci nesmí aplikaci shodit.
pub fn by_id(id: &str) -> (usize, &'static Palette) {
    PALETTES
        .iter()
        .position(|palette| palette.id == id)
        .map(|at| (at, &PALETTES[at]))
        .unwrap_or((0, &PALETTES[0]))
}

pub fn apply(ctx: &egui::Context, p: &Palette) {
    let mut visuals = egui::Visuals::dark();
    visuals.panel_fill = p.panel;
    visuals.window_fill = p.panel;
    visuals.extreme_bg_color = p.well;
    visuals.faint_bg_color = p.tile;
    visuals.code_bg_color = p.well;
    visuals.override_text_color = Some(p.text);
    visuals.hyperlink_color = p.accent;
    visuals.window_stroke = Stroke::new(1.0, p.bevel_light);
    visuals.selection.bg_fill = p.accent.gamma_multiply(0.45);
    visuals.selection.stroke = Stroke::new(1.0, p.accent);
    visuals.window_corner_radius = CornerRadius::same(4);

    for (widget, fill) in [
        (&mut visuals.widgets.noninteractive, p.panel),
        (&mut visuals.widgets.inactive, p.tile),
        (&mut visuals.widgets.hovered, p.caption),
        (&mut visuals.widgets.active, p.caption),
        (&mut visuals.widgets.open, p.tile),
    ] {
        widget.bg_fill = fill;
        widget.weak_bg_fill = fill;
        widget.bg_stroke = Stroke::new(1.0, p.bevel_dark);
        widget.fg_stroke = Stroke::new(1.0, p.text);
        widget.corner_radius = CornerRadius::same(3);
    }
    visuals.widgets.hovered.bg_stroke = Stroke::new(1.0, p.bevel_light);

    ctx.set_visuals(visuals);
}

/// Výška proužku s názvem.
pub const CAPTION: f32 = 22.0;
/// Kolik místa nechá rám kolem fotky.
pub const PADDING: f32 = 7.0;

/// Nakreslí diapozitiv a vrátí obdélník, do kterého patří fotka.
pub fn slide(
    painter: &egui::Painter,
    rect: Rect,
    p: &Palette,
    name: &str,
    selected: bool,
    hovered: bool,
) -> Rect {
    let radius = CornerRadius::same(2);
    let fill = if selected {
        p.tile.lerp_to_gamma(p.accent, 0.22)
    } else if hovered {
        p.tile.lerp_to_gamma(p.bevel_light, 0.35)
    } else {
        p.tile
    };
    painter.rect_filled(rect, radius, fill);

    // Jeden pixel světla nahoře a vlevo, jeden pixel stínu dole a vpravo.
    // Víc by z toho udělalo tlačítko.
    for (from, to, color) in [
        (
            rect.left_top() + Vec2::new(1.0, 0.5),
            rect.right_top() + Vec2::new(-1.0, 0.5),
            p.bevel_light,
        ),
        (
            rect.left_top() + Vec2::new(0.5, 1.0),
            rect.left_bottom() + Vec2::new(0.5, -1.0),
            p.bevel_light,
        ),
        (
            rect.left_bottom() + Vec2::new(1.0, -0.5),
            rect.right_bottom() + Vec2::new(-1.0, -0.5),
            p.bevel_dark,
        ),
        (
            rect.right_top() + Vec2::new(-0.5, 1.0),
            rect.right_bottom() + Vec2::new(-0.5, -1.0),
            p.bevel_dark,
        ),
    ] {
        painter.line_segment([from, to], Stroke::new(1.0, color));
    }

    if selected {
        painter.rect(
            rect,
            radius,
            Color32::TRANSPARENT,
            Stroke::new(1.0, p.accent),
            StrokeKind::Inside,
        );
    }

    let inner = rect.shrink(PADDING);
    let well = Rect::from_min_max(inner.min, egui::pos2(inner.max.x, inner.max.y - CAPTION));
    painter.rect_filled(well, CornerRadius::same(1), p.well);

    let strip = Rect::from_min_max(egui::pos2(inner.min.x, well.max.y + 2.0), inner.max);
    painter.rect_filled(
        strip,
        CornerRadius::same(1),
        if selected {
            p.caption.lerp_to_gamma(p.accent, 0.30)
        } else {
            p.caption
        },
    );

    // Jeden řádek s výpustkou, ne zalomení: název musí zůstat na proužku.
    let color = if selected { p.text } else { p.dim };
    let mut job = egui::text::LayoutJob::simple_singleline(
        name.to_owned(),
        FontId::proportional(11.0),
        color,
    );
    job.wrap = egui::text::TextWrapping {
        max_width: strip.width(),
        max_rows: 1,
        break_anywhere: true,
        overflow_character: Some('…'),
    };
    let galley = painter.layout_job(job);
    let at = egui::pos2(
        strip.center().x - galley.size().x * 0.5,
        strip.center().y - galley.size().y * 0.5,
    );
    painter.galley(at, galley, color);

    well
}

/// Obdélník pro obrázek o daném poměru stran vepsaný doprostřed plochy.
pub fn fit(area: Rect, size: [usize; 2]) -> Rect {
    let (w, h) = (size[0].max(1) as f32, size[1].max(1) as f32);
    let scale = (area.width() / w).min(area.height() / h);
    Rect::from_center_size(area.center(), Vec2::new(w * scale, h * scale))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn klice_palet_jsou_jedinecne() {
        let mut ids: Vec<_> = PALETTES.iter().map(|p| p.id).collect();
        let count = ids.len();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), count);
    }

    #[test]
    fn neznamy_motiv_spadne_na_prvni_misto_paniky() {
        assert_eq!(by_id("neexistuje").0, 0);
        assert_eq!(by_id("sepie").1.id, "sepie");
    }

    #[test]
    fn vychozi_motiv_z_nastaveni_existuje() {
        let default = photosite_core::Config::default().appearance.theme;
        assert_eq!(by_id(&default).1.id, default);
    }
}
