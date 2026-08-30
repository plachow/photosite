//! Převod motivu z jádra do egui a kreslení dlaždice.
//!
//! Barvy ani rozměry tu nejsou. Barvy přicházejí z [`photosite_core::theme`]
//! jako data, rozměry z nastavení. Tenhle soubor umí jediné: vzít je a
//! nakreslit podle nich.

use egui::{Color32, CornerRadius, FontId, Rect, Stroke, StrokeKind, Vec2};
use photosite_core::settings::Gallery;
use photosite_core::theme::{Color, Palette};

pub fn color(value: Color) -> Color32 {
    Color32::from_rgb(value.r, value.g, value.b)
}

/// Prožene paletu skrz egui.
///
/// Nastavuje se **každý** barevný slot, který egui má. Co se nechá být, to si
/// toolkit dokreslí po svém, a jeho výchozí barvy se s cizí paletou pohádají —
/// tak vzniká tmavě šedý text na šedém pozadí, který se pak hledá po jednom.
///
/// `override_text_color` se schválně nepoužívá: přebilo by veškerý text jednou
/// barvou a zrušilo rozdíl mezi běžným, druhotným a zakázaným.
pub fn apply(ctx: &egui::Context, palette: &Palette, dark: bool, scale: f32) {
    let mut visuals = if dark {
        egui::Visuals::dark()
    } else {
        egui::Visuals::light()
    };

    // Plochy
    visuals.panel_fill = color(palette.panel);
    visuals.window_fill = color(palette.window);
    visuals.extreme_bg_color = color(palette.well);
    visuals.faint_bg_color = color(palette.tile);
    visuals.code_bg_color = color(palette.well);
    visuals.text_edit_bg_color = Some(color(palette.well));

    // Text
    visuals.override_text_color = None;
    visuals.weak_text_color = Some(color(palette.dim));
    visuals.weak_text_alpha = 1.0;
    visuals.warn_fg_color = color(palette.warn);
    visuals.error_fg_color = color(palette.error);
    visuals.hyperlink_color = color(palette.accent);

    // Rámy a výběr
    visuals.window_stroke = egui::Stroke::new(1.0, color(palette.bevel_light));
    visuals.window_corner_radius = CornerRadius::same(4);
    visuals.menu_corner_radius = CornerRadius::same(4);
    visuals.selection.bg_fill = color(palette.accent).gamma_multiply(0.45);
    visuals.selection.stroke = egui::Stroke::new(1.0, color(palette.text));

    for (widget, fill, hrana) in [
        (
            &mut visuals.widgets.noninteractive,
            palette.panel,
            palette.bevel_dark,
        ),
        (
            &mut visuals.widgets.inactive,
            palette.tile,
            palette.bevel_dark,
        ),
        (
            &mut visuals.widgets.hovered,
            palette.caption,
            palette.bevel_light,
        ),
        (&mut visuals.widgets.active, palette.caption, palette.accent),
        (&mut visuals.widgets.open, palette.tile, palette.bevel_light),
    ] {
        widget.bg_fill = color(fill);
        widget.weak_bg_fill = color(fill);
        widget.bg_stroke = egui::Stroke::new(1.0, color(hrana));
        widget.fg_stroke = egui::Stroke::new(1.0, color(palette.text));
        widget.corner_radius = CornerRadius::same(3);
    }

    // Do obou slotů, ne jen do aktivního.
    //
    // `set_visuals` zapisuje pod motiv, který je zrovna zvolený. Při startu
    // systém ještě nestihl ohlásit, jestli je v tmavém nebo světlém režimu,
    // takže egui použije tmavý; jakmile odpověď dorazí a je „světlo", přepne
    // na světlý slot — a v něm jsou pořád jeho vlastní barvy. Okno nastavení
    // pak svítí bíle uprostřed tmavé aplikace.
    ctx.set_visuals_of(egui::Theme::Dark, visuals.clone());
    ctx.set_visuals_of(egui::Theme::Light, visuals);
    ctx.set_theme(if dark {
        egui::ThemePreference::Dark
    } else {
        egui::ThemePreference::Light
    });
    ctx.set_pixels_per_point(scale.clamp(0.5, 3.0));
}

/// Výška proužku s názvem; nula, když se popisky nezobrazují.
pub fn caption_height(gallery: &Gallery) -> f32 {
    if gallery.show_captions {
        gallery.caption_height as f32
    } else {
        0.0
    }
}

/// Výška dlaždice podle nastavení: obrázek, proužek a rám.
pub fn tile_height(gallery: &Gallery) -> f32 {
    (gallery.tile_size * gallery.tile_aspect) as f32
        + caption_height(gallery)
        + gallery.tile_padding as f32
}

/// Nakreslí diapozitiv a vrátí obdélník, do kterého patří fotka.
pub fn slide(
    painter: &egui::Painter,
    rect: Rect,
    palette: &Palette,
    gallery: &Gallery,
    name: &str,
    selected: bool,
    hovered: bool,
) -> Rect {
    let radius = CornerRadius::same(2);
    let tile = color(palette.tile);
    let fill = if selected {
        tile.lerp_to_gamma(color(palette.accent), 0.22)
    } else if hovered {
        tile.lerp_to_gamma(color(palette.bevel_light), 0.35)
    } else {
        tile
    };
    painter.rect_filled(rect, radius, fill);

    // Jeden pixel světla nahoře a vlevo, jeden pixel stínu dole a vpravo.
    // Víc by z toho udělalo tlačítko.
    for (from, to, line) in [
        (
            rect.left_top() + Vec2::new(1.0, 0.5),
            rect.right_top() + Vec2::new(-1.0, 0.5),
            palette.bevel_light,
        ),
        (
            rect.left_top() + Vec2::new(0.5, 1.0),
            rect.left_bottom() + Vec2::new(0.5, -1.0),
            palette.bevel_light,
        ),
        (
            rect.left_bottom() + Vec2::new(1.0, -0.5),
            rect.right_bottom() + Vec2::new(-1.0, -0.5),
            palette.bevel_dark,
        ),
        (
            rect.right_top() + Vec2::new(-0.5, 1.0),
            rect.right_bottom() + Vec2::new(-0.5, -1.0),
            palette.bevel_dark,
        ),
    ] {
        painter.line_segment([from, to], Stroke::new(1.0, color(line)));
    }

    if selected {
        painter.rect(
            rect,
            radius,
            Color32::TRANSPARENT,
            Stroke::new(1.0, color(palette.accent)),
            StrokeKind::Inside,
        );
    }

    let inner = rect.shrink(gallery.tile_padding as f32);
    let strip_height = caption_height(gallery);
    let well = Rect::from_min_max(
        inner.min,
        egui::pos2(inner.max.x, inner.max.y - strip_height),
    );
    painter.rect_filled(well, CornerRadius::same(1), color(palette.well));

    if !gallery.show_captions {
        return well;
    }

    let strip = Rect::from_min_max(egui::pos2(inner.min.x, well.max.y + 2.0), inner.max);
    painter.rect_filled(
        strip,
        CornerRadius::same(1),
        if selected {
            color(palette.caption).lerp_to_gamma(color(palette.accent), 0.30)
        } else {
            color(palette.caption)
        },
    );

    // Jeden řádek s výpustkou, ne zalomení: název musí zůstat na proužku.
    let text = if selected { palette.text } else { palette.dim };
    let mut job = egui::text::LayoutJob::simple_singleline(
        name.to_owned(),
        FontId::proportional(11.0),
        color(text),
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
    painter.galley(at, galley, color(text));

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
    use photosite_core::theme::THEMES;

    #[test]
    fn prevod_barvy_nic_neztrati() {
        for theme in THEMES {
            let converted = color(theme.palette.accent);
            let original = theme.palette.accent;
            assert_eq!(
                (converted.r(), converted.g(), converted.b()),
                (original.r, original.g, original.b)
            );
        }
    }

    /// Zakázaný text nekreslí naše paleta, ale egui: vezme barvu textu a
    /// zamíchá ji směrem k `noninteractive.weak_bg_fill`. Tenhle test si tedy
    /// spočítá, co se doopravdy objeví na obrazovce, a změří to.
    ///
    /// Bez něj je „zakázané tlačítko je nečitelné" věc, na kterou se přijde
    /// očima, a to je přesně to lovení, kterému se chceme vyhnout.
    #[test]
    fn zakazany_text_zustane_citelny_i_po_egui() {
        for theme in THEMES {
            let p = &theme.palette;
            let mut style = egui::Style {
                visuals: if theme.dark {
                    egui::Visuals::dark()
                } else {
                    egui::Visuals::light()
                },
                ..Default::default()
            };
            style.visuals.widgets.noninteractive.weak_bg_fill = color(p.panel);

            let vysledek = style.visuals.gray_out(color(p.text));
            let jako_barva = photosite_core::theme::Color {
                r: vysledek.r(),
                g: vysledek.g(),
                b: vysledek.b(),
            };
            let pomer = photosite_core::theme::contrast(jako_barva, p.panel);
            assert!(
                pomer >= 2.0,
                "{}: zakázaný text vyjde na kontrast {pomer:.2} ({} na {})",
                theme.id,
                jako_barva.to_hex(),
                p.panel.to_hex()
            );
        }
    }

    #[test]
    fn vyska_dlazdice_reaguje_na_nastaveni() {
        let mut gallery = Gallery::default();
        let s_popisky = tile_height(&gallery);
        gallery.show_captions = false;
        let bez = tile_height(&gallery);
        assert!(
            bez < s_popisky,
            "bez popisků musí být dlaždice nižší: {bez} proti {s_popisky}"
        );

        gallery.show_captions = true;
        gallery.tile_size *= 2.0;
        assert!(tile_height(&gallery) > s_popisky);
    }
}
