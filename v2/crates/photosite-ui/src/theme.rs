//! Carrying a theme from the core into egui, and drawing a tile.
//!
//! Neither colours nor dimensions live here. Colours arrive from
//! [`photosite_core::theme`] as data, dimensions from the settings. This file
//! does one thing: take them and draw by them.

use egui::{Color32, CornerRadius, FontId, Rect, Stroke, StrokeKind, Vec2};
use photosite_core::domain::{Flag, Organisation};
use photosite_core::place::Verdict;
use photosite_core::settings::Gallery;
use photosite_core::theme::{Color, Palette};

pub fn color(value: Color) -> Color32 {
    Color32::from_rgb(value.r, value.g, value.b)
}

/// Runs the palette through egui.
///
/// **Every** colour slot egui has is set. Whatever is left alone the toolkit
/// fills in its own way, and its defaults argue with a foreign palette — that
/// is how dark grey text on a grey background appears, to be hunted down one
/// instance at a time afterwards.
///
/// `override_text_color` is deliberately unused: it would force one colour on
/// all text and erase the difference between ordinary, secondary and
/// disabled.
pub fn apply(ctx: &egui::Context, palette: &Palette, dark: bool, scale: f32) {
    let mut visuals = if dark {
        egui::Visuals::dark()
    } else {
        egui::Visuals::light()
    };

    // Surfaces
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

    // Frames and selection
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

    // Into both slots, not only the active one.
    //
    // `set_visuals` writes under whichever theme is currently chosen. At
    // startup the system has not yet said whether it is in dark or light
    // mode, so egui uses dark; the moment the answer arrives and says
    // "light", it switches to the light slot — which still holds its own
    // colours. The settings window then glows white in the middle of a dark
    // application.
    ctx.set_visuals_of(egui::Theme::Dark, visuals.clone());
    ctx.set_visuals_of(egui::Theme::Light, visuals);
    ctx.set_theme(if dark {
        egui::ThemePreference::Dark
    } else {
        egui::ThemePreference::Light
    });
    ctx.set_pixels_per_point(scale.clamp(0.5, 3.0));
}

/// Height of the caption strip; zero when captions are not shown.
pub fn caption_height(gallery: &Gallery) -> f32 {
    if gallery.show_captions {
        gallery.caption_height as f32
    } else {
        0.0
    }
}

/// Tile height from the settings: image, caption strip and frame.
pub fn tile_height(gallery: &Gallery) -> f32 {
    (gallery.tile_size * gallery.tile_aspect) as f32
        + caption_height(gallery)
        + gallery.tile_padding as f32
}

/// A little triangle, pointing down or up.
///
/// Drawn and not written, for the same reason as the star and as the folder
/// tree's own: the default font has neither `▲` nor `▼` and an absent glyph
/// comes out as an empty box.
pub fn caret(painter: &egui::Painter, centre: egui::Pos2, down: bool, fill: Color32) {
    let reach = 4.0;
    let points = if down {
        vec![
            egui::pos2(centre.x - reach, centre.y - reach * 0.5),
            egui::pos2(centre.x + reach, centre.y - reach * 0.5),
            egui::pos2(centre.x, centre.y + reach * 0.75),
        ]
    } else {
        vec![
            egui::pos2(centre.x - reach, centre.y + reach * 0.5),
            egui::pos2(centre.x + reach, centre.y + reach * 0.5),
            egui::pos2(centre.x, centre.y - reach * 0.75),
        ]
    };
    painter.add(egui::Shape::convex_polygon(points, fill, Stroke::NONE));
}

/// A five-pointed star, drawn rather than written.
///
/// The default font has no `★` — the same reason the folder tree draws its
/// own triangles. A glyph that is not there comes out as an empty box, and a
/// rating is the last thing that should be guesswork.
///
/// Ten points around the centre, filled as a fan. A star is concave, so a
/// convex polygon would fill it wrong.
pub fn star(painter: &egui::Painter, centre: egui::Pos2, radius: f32, fill: Color32) {
    use std::f32::consts::{FRAC_PI_2, PI};

    let mut mesh = egui::Mesh::default();
    mesh.colored_vertex(centre, fill);
    for point in 0..10 {
        let angle = -FRAC_PI_2 + point as f32 * PI / 5.0;
        let reach = if point % 2 == 0 {
            radius
        } else {
            radius * 0.42
        };
        mesh.colored_vertex(centre + Vec2::angled(angle) * reach, fill);
    }

    for point in 0..10u32 {
        mesh.add_triangle(0, 1 + point, 1 + (point + 1) % 10);
    }

    painter.add(egui::Shape::mesh(mesh));
}

/// The colour of a verdict about a position.
///
/// Its own colours and not the palette's, for the same reason the label
/// swatch has its own: a traffic light that is not green, amber and red is
/// not a traffic light. `None` for a verdict worth no mark at all.
pub fn verdict_color(verdict: Verdict) -> Option<Color32> {
    match verdict {
        Verdict::Nowhere => None,
        Verdict::Precise => Some(Color32::from_rgb(0x4C, 0xAF, 0x50)),
        Verdict::Approximate => Some(Color32::from_rgb(0xF2, 0xA3, 0x3A)),
        Verdict::Doubtful => Some(Color32::from_rgb(0xE0, 0x5A, 0x4C)),
    }
}

/// A map pin, drawn rather than written.
///
/// v1 used an emoji and got away with it on one platform. A shape needs no
/// font to be installed and no fallback to be right.
pub fn pin(painter: &egui::Painter, centre: egui::Pos2, radius: f32, fill: Color32) {
    let head = egui::pos2(centre.x, centre.y - radius * 0.25);
    painter.circle_filled(head, radius * 0.75, fill);
    painter.add(egui::Shape::convex_polygon(
        vec![
            egui::pos2(head.x - radius * 0.55, head.y + radius * 0.45),
            egui::pos2(head.x + radius * 0.55, head.y + radius * 0.45),
            egui::pos2(head.x, centre.y + radius),
        ],
        fill,
        Stroke::NONE,
    ));
}

/// What somebody said about a photograph, over the photograph.
///
/// Everything sits on a dark plate rather than straight on the picture. A
/// rating drawn in the palette's own colours disappears against a bright sky
/// or a dark forest depending on the theme, and which of the two is pure
/// chance.
///
/// A tile nobody has said anything about gets nothing drawn at all, which is
/// most tiles in most libraries.
pub fn badges(
    painter: &egui::Painter,
    well: Rect,
    palette: &Palette,
    organisation: &Organisation,
    verdict: Verdict,
    people: &[photosite_core::people::Tag],
    expressions: photosite_core::people::Expressions,
) {
    // A rejected photograph is still there — it only fades. Deleting is a
    // separate, deliberate step, and dimming is what says so.
    if organisation.flag == Flag::Rejected {
        painter.rect_filled(well, CornerRadius::ZERO, Color32::from_black_alpha(150));
    }

    let size = (well.height() * 0.09).clamp(5.0, 11.0);
    let pad = size * 0.6;

    // A doubted position, and only a doubted one. A pin on every photograph
    // that carries coordinates would be on most of a phone's library and
    // would say nothing; this one means "worth a look".
    if verdict.is_doubted()
        && let Some(fill) = verdict_color(verdict)
    {
        let side = size * 2.0;
        let spot = Rect::from_min_size(
            egui::pos2(well.max.x - pad - side, well.max.y - pad - side),
            Vec2::splat(side),
        );
        painter.rect_filled(spot, CornerRadius::same(2), Color32::from_black_alpha(120));
        pin(painter, spot.center(), size * 0.8, fill);
    }

    // Who is on it, as a row of dots in their own colours. No names: at
    // three pixels high a name is a smudge, and the colour is what says
    // "the same person as on that other tile".
    if !people.is_empty() {
        let dot = size * 1.4;
        let plate = Rect::from_min_size(
            egui::pos2(
                well.min.x + pad,
                well.max.y - pad - size * 2.2 - dot - pad * 0.5,
            ),
            Vec2::new(
                (dot + pad * 0.4) * people.len().min(6) as f32 + pad * 0.6,
                dot + pad * 0.6,
            ),
        );
        painter.rect_filled(plate, CornerRadius::same(2), Color32::from_black_alpha(120));
        for (index, tag) in people.iter().take(6).enumerate() {
            let colour = photosite_core::theme::person_color(tag.id);
            let centre = egui::pos2(
                plate.min.x + pad * 0.3 + dot / 2.0 + index as f32 * (dot + pad * 0.4),
                plate.center().y,
            );
            painter.circle_filled(
                centre,
                dot / 2.0,
                Color32::from_rgb(colour.r, colour.g, colour.b),
            );
        }
    }

    // A quiet mark for a photograph worth a second look: somebody blinked,
    // or somebody is not smiling. Nothing at all when everybody passes —
    // a badge on every good photograph is a badge that says nothing.
    if expressions.worth_showing() {
        let side = size * 2.0;
        let spot = Rect::from_min_size(
            egui::pos2(
                well.max.x - pad - side,
                well.max.y - pad - side - side - pad * 0.5,
            ),
            Vec2::splat(side),
        );
        painter.rect_filled(spot, CornerRadius::same(2), Color32::from_black_alpha(120));
        let amber = Color32::from_rgb(0xF2, 0xC5, 0x4E);
        if expressions.anyone_blinking() {
            closed_eye(painter, spot.center(), size * 0.75, amber);
        } else {
            frown(painter, spot.center(), size * 0.75, amber);
        }
    }

    if organisation.is_empty() {
        return;
    }

    if organisation.rating > 0 {
        let width = size * 2.0 * Organisation::MAX_RATING as f32 + pad;
        let plate = Rect::from_min_size(
            egui::pos2(well.min.x + pad, well.max.y - pad - size * 2.2),
            Vec2::new(width, size * 2.2),
        );
        painter.rect_filled(plate, CornerRadius::same(2), Color32::from_black_alpha(120));

        for index in 0..Organisation::MAX_RATING {
            let centre = egui::pos2(
                plate.min.x + pad * 0.5 + size + index as f32 * size * 2.0,
                plate.center().y,
            );
            let lit = index < organisation.rating;
            star(
                painter,
                centre,
                size * 0.9,
                if lit {
                    Color32::from_rgb(0xF2, 0xC5, 0x4E)
                } else {
                    Color32::from_white_alpha(60)
                },
            );
        }
    }

    // The label goes in the corner as its own colour, never the palette's —
    // red has to look red in every theme or the word and the colour stop
    // agreeing.
    if let Some(swatch) = organisation.label.color() {
        let side = size * 2.0;
        let spot = Rect::from_min_size(
            egui::pos2(well.max.x - pad - side, well.min.y + pad),
            Vec2::splat(side),
        );
        painter.rect_filled(spot, CornerRadius::same(2), Color32::from_black_alpha(120));
        painter.rect_filled(
            spot.shrink(1.5),
            CornerRadius::same(2),
            Color32::from_rgb(swatch.r, swatch.g, swatch.b),
        );
    }

    if organisation.flag == Flag::Picked {
        let side = size * 2.0;
        let spot = Rect::from_min_size(
            egui::pos2(well.min.x + pad, well.min.y + pad),
            Vec2::splat(side),
        );
        painter.rect_filled(spot, CornerRadius::same(2), Color32::from_black_alpha(120));
        // A tick, drawn for the same reason as the star.
        let centre = spot.center();
        let arm = side * 0.28;
        painter.add(egui::Shape::line(
            vec![
                egui::pos2(centre.x - arm, centre.y),
                egui::pos2(centre.x - arm * 0.25, centre.y + arm * 0.8),
                egui::pos2(centre.x + arm, centre.y - arm * 0.7),
            ],
            Stroke::new((size * 0.28).max(1.5), color(palette.accent)),
        ));
    }
}

/// A shut eye: a shallow arc with two lashes under it.
///
/// Drawn rather than written, for the same reason the stars are — the
/// default font has no such glyph, and a font that did would be a font to
/// ship.
fn closed_eye(painter: &egui::Painter, centre: egui::Pos2, size: f32, colour: Color32) {
    let stroke = Stroke::new((size * 0.32).max(1.2), colour);
    painter.add(egui::Shape::line(arc(centre, size, -0.35, false), stroke));
    for side in [-1.0f32, 1.0] {
        painter.line_segment(
            [
                egui::pos2(centre.x + side * size * 0.55, centre.y + size * 0.12),
                egui::pos2(centre.x + side * size * 0.75, centre.y + size * 0.55),
            ],
            stroke,
        );
    }
}

/// A mouth turned down.
fn frown(painter: &egui::Painter, centre: egui::Pos2, size: f32, colour: Color32) {
    let stroke = Stroke::new((size * 0.32).max(1.2), colour);
    painter.add(egui::Shape::line(
        arc(centre + Vec2::new(0.0, size * 0.35), size, 0.5, true),
        stroke,
    ));
}

/// Points along a shallow parabola: `depth` says how far it bends and which
/// way, `up` which side of the middle it sits.
fn arc(centre: egui::Pos2, size: f32, depth: f32, up: bool) -> Vec<egui::Pos2> {
    let sign = if up { -1.0 } else { 1.0 };
    (0..=8)
        .map(|step| {
            let along = step as f32 / 8.0 * 2.0 - 1.0;
            egui::pos2(
                centre.x + along * size,
                centre.y + sign * depth * size * (1.0 - along * along),
            )
        })
        .collect()
}

/// Draws the slide and returns the rectangle the photograph belongs in.
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

    // One pixel of light at the top and left, one pixel of shadow at the
    // bottom and right. More would make a button of it.
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

    // One line with an ellipsis, not a wrap: the name has to stay on the
    // strip.
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

/// A rectangle for an image of the given aspect ratio, inscribed in the
/// middle of the area.
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
    fn converting_a_colour_loses_nothing() {
        for theme in THEMES {
            let converted = color(theme.palette.accent);
            let original = theme.palette.accent;
            assert_eq!(
                (converted.r(), converted.g(), converted.b()),
                (original.r, original.g, original.b)
            );
        }
    }

    /// Disabled text is not drawn by our palette but by egui: it takes the
    /// text colour and blends it toward `noninteractive.weak_bg_fill`. So
    /// this test works out what actually appears on screen and measures that.
    ///
    /// Without it, "the disabled button is unreadable" is something found by
    /// eye, and that is exactly the hunting we are trying to avoid.
    #[test]
    fn disabled_text_stays_legible_even_after_egui() {
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

            let result = style.visuals.gray_out(color(p.text));
            let as_colour = photosite_core::theme::Color {
                r: result.r(),
                g: result.g(),
                b: result.b(),
            };
            let ratio = photosite_core::theme::contrast(as_colour, p.panel);
            assert!(
                ratio >= 2.0,
                "{}: disabled text comes out at contrast {ratio:.2} ({} on {})",
                theme.id,
                as_colour.to_hex(),
                p.panel.to_hex()
            );
        }
    }

    #[test]
    fn tile_height_follows_the_settings() {
        let mut gallery = Gallery::default();
        let with = tile_height(&gallery);
        gallery.show_captions = false;
        let without = tile_height(&gallery);
        assert!(
            without < with,
            "without captions the tile has to be shorter: {without} against {with}"
        );

        gallery.show_captions = true;
        gallery.tile_size *= 2.0;
        assert!(tile_height(&gallery) > with);
    }
}
