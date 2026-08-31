//! Themes as data — and a test that keeps them legible.
//!
//! Colours are not constants in code but values that can be serialised. Today
//! they are built in, but precisely because they are data they will one day
//! be loadable from a file without touching anything in the code.
//!
//! The palette covers **every** role the drawing layer needs, disabled text,
//! warnings and errors included. Whatever the palette does not settle, the
//! toolkit fills in its own way — and its defaults argue with a foreign
//! palette. That is how dark grey text on a grey background appears, to be
//! hunted down one instance at a time.
//!
//! So that it need not be hunted, there is [`contrast`] and a test that walks
//! **every theme times every foreground-background pair**. An illegible
//! combination is from now on a failing test, not a report from a user.
//!
//! The core knows of no graphics library — [`Color`] is three bytes.

use crate::settings::Appearance;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// A colour in the form a person can read and write: `#2A2A2C`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Color {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl Color {
    pub const fn hex(value: u32) -> Self {
        Self {
            r: ((value >> 16) & 0xFF) as u8,
            g: ((value >> 8) & 0xFF) as u8,
            b: (value & 0xFF) as u8,
        }
    }

    pub fn to_hex(self) -> String {
        format!("#{:02X}{:02X}{:02X}", self.r, self.g, self.b)
    }

    pub fn parse(text: &str) -> Option<Self> {
        let digits = text.strip_prefix('#').unwrap_or(text);
        if digits.len() != 6 {
            return None;
        }

        Some(Self {
            r: u8::from_str_radix(&digits[0..2], 16).ok()?,
            g: u8::from_str_radix(&digits[2..4], 16).ok()?,
            b: u8::from_str_radix(&digits[4..6], 16).ok()?,
        })
    }

    /// Relative luminance by WCAG. The basis for [`contrast`].
    pub fn luminance(self) -> f64 {
        fn channel(value: u8) -> f64 {
            let c = value as f64 / 255.0;
            if c <= 0.039_28 {
                c / 12.92
            } else {
                ((c + 0.055) / 1.055).powf(2.4)
            }
        }

        0.2126 * channel(self.r) + 0.7152 * channel(self.g) + 0.0722 * channel(self.b)
    }
}

/// The contrast ratio of two colours, 1.0 to 21.0.
///
/// At least 4.5 is recommended for ordinary text; secondary and disabled text
/// can do with less, but never so little that the text merges into the
/// background.
pub fn contrast(a: Color, b: Color) -> f64 {
    let (first, second) = (a.luminance(), b.luminance());
    let (lighter, darker) = if first > second {
        (first, second)
    } else {
        (second, first)
    };
    (lighter + 0.05) / (darker + 0.05)
}

impl Serialize for Color {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_hex())
    }
}

impl<'de> Deserialize<'de> for Color {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        Color::parse(&text).ok_or_else(|| {
            serde::de::Error::custom(format!("{text:?} is not a colour like #RRGGBB"))
        })
    }
}

/// One theme's set of colours.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Palette {
    /// The background behind everything.
    pub window: Color,
    /// Docks and bars.
    pub panel: Color,
    /// The slide frame.
    pub tile: Color,
    /// The surface under the photograph. Darker than the frame, so the
    /// photograph sits in a window.
    pub well: Color,
    /// The caption strip.
    pub caption: Color,
    /// Primary text.
    pub text: Color,
    /// Secondary text: labels, paths, the status bar.
    pub dim: Color,
    /// The text of a disabled control. Disabled does not mean invisible.
    pub disabled: Color,
    pub accent: Color,
    /// Warnings and errors. Without them the toolkit would draw its own.
    pub warn: Color,
    pub error: Color,
    /// The top and left edge of the frame. A hint only, not a raised button.
    pub bevel_light: Color,
    pub bevel_dark: Color,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Theme {
    /// The settings key. Never changes.
    pub id: &'static str,
    /// A translation key. There is no text here either.
    pub label_key: &'static str,
    /// Is this a dark theme? The automatic choice goes by this.
    pub dark: bool,
    pub palette: Palette,
}

/// The value of `appearance.theme` that means "follow the system".
pub const AUTOMATIC: &str = "automatic";

pub const THEMES: &[Theme] = &[
    Theme {
        id: "dark",
        label_key: "theme-dark",
        dark: true,
        palette: Palette {
            window: Color::hex(0x22_2224),
            panel: Color::hex(0x2A_2A2C),
            tile: Color::hex(0x3A_3A3D),
            well: Color::hex(0x1A_1A1C),
            caption: Color::hex(0x44_4448),
            text: Color::hex(0xDA_DADE),
            dim: Color::hex(0x9A_9AA2),
            disabled: Color::hex(0x6E_6E76),
            accent: Color::hex(0x5B_9DD9),
            warn: Color::hex(0xE0_B15A),
            error: Color::hex(0xE0_705A),
            bevel_light: Color::hex(0x50_5055),
            bevel_dark: Color::hex(0x16_1618),
        },
    },
    Theme {
        id: "light",
        label_key: "theme-light",
        dark: false,
        palette: Palette {
            window: Color::hex(0xE4_E4E6),
            panel: Color::hex(0xD8_D8DA),
            tile: Color::hex(0xF0_F0F2),
            well: Color::hex(0xBE_BEC2),
            caption: Color::hex(0xE0_E0E4),
            text: Color::hex(0x1E_1E20),
            dim: Color::hex(0x66_666C),
            disabled: Color::hex(0x85_858C),
            accent: Color::hex(0x1F_6FB2),
            warn: Color::hex(0x8A_5A00),
            error: Color::hex(0xB0_201A),
            bevel_light: Color::hex(0xFF_FFFF),
            bevel_dark: Color::hex(0xA8_A8AC),
        },
    },
    Theme {
        id: "grey",
        label_key: "theme-grey",
        dark: true,
        palette: Palette {
            window: Color::hex(0x3C_3C3E),
            panel: Color::hex(0x46_4648),
            tile: Color::hex(0x58_585B),
            well: Color::hex(0x2E_2E30),
            caption: Color::hex(0x62_6266),
            text: Color::hex(0xEC_ECEE),
            dim: Color::hex(0xC0_C0C6),
            disabled: Color::hex(0x85_858A),
            accent: Color::hex(0x6F_B0E8),
            warn: Color::hex(0xE8_BC66),
            error: Color::hex(0xE8_7A66),
            bevel_light: Color::hex(0x74_7478),
            bevel_dark: Color::hex(0x2A_2A2C),
        },
    },
    Theme {
        id: "sepia",
        label_key: "theme-sepia",
        dark: true,
        palette: Palette {
            window: Color::hex(0x26_211B),
            panel: Color::hex(0x2E_2821),
            tile: Color::hex(0x40_382D),
            well: Color::hex(0x1B_1712),
            caption: Color::hex(0x4C_4235),
            text: Color::hex(0xE4_D8C4),
            dim: Color::hex(0xA9_9C85),
            disabled: Color::hex(0x7B_705C),
            accent: Color::hex(0xC9_944F),
            warn: Color::hex(0xD7_A24A),
            error: Color::hex(0xD2_705A),
            bevel_light: Color::hex(0x58_4D3E),
            bevel_dark: Color::hex(0x18_1410),
        },
    },
    Theme {
        id: "seabreeze",
        label_key: "theme-seabreeze",
        dark: true,
        palette: Palette {
            window: Color::hex(0x17_2029),
            panel: Color::hex(0x1D_2833),
            tile: Color::hex(0x2A_3947),
            well: Color::hex(0x10_171E),
            caption: Color::hex(0x33_4557),
            text: Color::hex(0xD6_E4EE),
            dim: Color::hex(0x8E_A6B8),
            disabled: Color::hex(0x64_7A8C),
            accent: Color::hex(0x4F_C3D9),
            warn: Color::hex(0xE0_B96F),
            error: Color::hex(0xE0_8A7A),
            bevel_light: Color::hex(0x3C_5064),
            bevel_dark: Color::hex(0x0C_1218),
        },
    },
];

pub fn theme(id: &str) -> Option<&'static Theme> {
    THEMES.iter().find(|theme| theme.id == id)
}

/// Which theme to use.
///
/// `system_dark` is what the system reports — `None` means it could not be
/// asked. An unknown theme name falls back to the first in the list, not to a
/// panic; a typo in the settings must not rob the application of its
/// colours.
pub fn resolve(appearance: &Appearance, system_dark: Option<bool>) -> &'static Theme {
    if appearance.theme == AUTOMATIC {
        let wanted = if system_dark.unwrap_or(true) {
            &appearance.theme_dark
        } else {
            &appearance.theme_light
        };
        return theme(wanted).unwrap_or(&THEMES[0]);
    }

    theme(&appearance.theme).unwrap_or(&THEMES[0])
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The smallest acceptable contrast for a given role of text.
    const PRIMARY: f64 = 4.5;
    const SECONDARY: f64 = 3.0;
    const DISABLED: f64 = 2.2;

    #[test]
    fn a_colour_there_and_back() {
        for text in ["#000000", "#FFFFFF", "#2A2A2C", "#4FC3D9"] {
            assert_eq!(Color::parse(text).unwrap().to_hex(), text);
        }

        assert_eq!(Color::parse("2A2A2C"), Color::parse("#2A2A2C"));
        assert_eq!(Color::parse("#ZZZZZZ"), None);
        assert_eq!(Color::parse("#FFF"), None);
    }

    #[test]
    fn kontrast_pocita_spravne() {
        let black = Color::hex(0x00_0000);
        let white = Color::hex(0xFF_FFFF);
        assert!((contrast(black, white) - 21.0).abs() < 0.01);
        assert!((contrast(white, white) - 1.0).abs() < 0.01);
        // The order does not matter.
        assert_eq!(contrast(black, white), contrast(white, black));
    }

    /// This is the test the whole thing was built for: walk every theme
    /// times every foreground-background pair. Dark grey text on a grey
    /// background is from now on a failing test, not a report from a user.
    #[test]
    fn every_theme_is_legible() {
        let mut sins = Vec::new();
        for theme in THEMES {
            let p = &theme.palette;
            let pairs: &[(&str, Color, Color, f64)] = &[
                ("text on the window", p.text, p.window, PRIMARY),
                ("text on a panel", p.text, p.panel, PRIMARY),
                ("text on a tile", p.text, p.tile, PRIMARY),
                ("text on the caption strip", p.text, p.caption, PRIMARY),
                ("text on the well", p.text, p.well, PRIMARY),
                ("secondary on the window", p.dim, p.window, SECONDARY),
                ("secondary on a panel", p.dim, p.panel, SECONDARY),
                (
                    "secondary on the caption strip",
                    p.dim,
                    p.caption,
                    SECONDARY,
                ),
                ("disabled on a panel", p.disabled, p.panel, DISABLED),
                ("disabled on the window", p.disabled, p.window, DISABLED),
                ("accent on a panel", p.accent, p.panel, SECONDARY),
                ("accent on a tile", p.accent, p.tile, SECONDARY),
                ("warning on a panel", p.warn, p.panel, SECONDARY),
                ("error on a panel", p.error, p.panel, SECONDARY),
            ];

            for (where_, foreground, background, threshold) in pairs {
                let ratio = contrast(*foreground, *background);
                if ratio < *threshold {
                    sins.push(format!(
                        "{}: {where_} has contrast {ratio:.2}, needs {threshold:.1} ({} on {})",
                        theme.id,
                        foreground.to_hex(),
                        background.to_hex()
                    ));
                }
            }
        }

        assert!(
            sins.is_empty(),
            "illegible combinations:\n  {}",
            sins.join("\n  ")
        );
    }

    /// Disabled text has to be weaker than ordinary text, but not invisible.
    /// Were it to merge with the primary, a disabled control could not be
    /// told from a live one.
    #[test]
    fn disabled_text_is_weaker_than_ordinary_but_still_visible() {
        for theme in THEMES {
            let p = &theme.palette;
            let ordinary = contrast(p.text, p.panel);
            let disabled = contrast(p.disabled, p.panel);
            assert!(
                disabled < ordinary,
                "{}: disabled text is not weaker ({disabled:.2} against {ordinary:.2})",
                theme.id
            );
            assert!(
                disabled >= DISABLED,
                "{}: disabled text is invisible ({disabled:.2})",
                theme.id
            );
        }
    }

    /// The hint of relief has to be visible without turning the frame into a
    /// button.
    #[test]
    fn the_bevel_shows_without_shouting() {
        for theme in THEMES {
            let p = &theme.palette;
            for (where_, edge) in [("light", p.bevel_light), ("dark", p.bevel_dark)] {
                let ratio = contrast(edge, p.tile);
                assert!(
                    ratio > 1.1,
                    "{}: the {where_} edge merges with the frame ({ratio:.2})",
                    theme.id
                );
                assert!(
                    ratio < 6.0,
                    "{}: the {where_} edge is too harsh ({ratio:.2})",
                    theme.id
                );
            }
        }
    }

    #[test]
    fn the_dark_flag_matches_the_colours() {
        for theme in THEMES {
            let light_background = theme.palette.window.luminance() > 0.35;
            assert_eq!(
                theme.dark, !light_background,
                "{}: the dark flag does not match the background colour",
                theme.id
            );
        }
    }

    #[test]
    fn a_theme_can_be_saved_and_loaded() {
        // This is the whole reason colours are data: a user theme will be
        // exactly this TOML in a file.
        let text = toml::to_string(&THEMES[0].palette).unwrap();
        assert!(text.contains("#22"), "{text}");
        let back: Palette = toml::from_str(&text).unwrap();
        assert_eq!(back, THEMES[0].palette);
    }

    #[test]
    fn theme_identifiers_are_unique() {
        let mut ids: Vec<_> = THEMES.iter().map(|theme| theme.id).collect();
        let count = ids.len();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), count);
    }

    #[test]
    fn every_theme_has_a_translation() {
        for theme in THEMES {
            assert!(crate::i18n::has(theme.label_key), "{}", theme.id);
        }
    }

    #[test]
    fn the_automatic_theme_follows_the_system() {
        let appearance = Appearance::default();
        assert_eq!(appearance.theme, AUTOMATIC);
        assert_eq!(resolve(&appearance, Some(true)).id, "dark");
        assert_eq!(resolve(&appearance, Some(false)).id, "light");
        // When the system cannot be asked, dark is the safer choice for
        // photographs.
        assert!(resolve(&appearance, None).dark);
    }

    #[test]
    fn an_explicit_theme_overrides_the_automatic_one() {
        let appearance = Appearance {
            theme: "sepia".to_owned(),
            ..Appearance::default()
        };
        assert_eq!(resolve(&appearance, Some(false)).id, "sepia");
    }

    #[test]
    fn a_typo_in_the_settings_does_not_lose_the_colours() {
        let appearance = Appearance {
            theme: "neexistuje".to_owned(),
            ..Appearance::default()
        };
        assert_eq!(resolve(&appearance, Some(true)).id, THEMES[0].id);
    }

    #[test]
    fn the_default_themes_for_automatic_mode_exist() {
        let appearance = Appearance::default();
        assert!(theme(&appearance.theme_dark).is_some());
        assert!(theme(&appearance.theme_light).is_some());
        assert!(theme(&appearance.theme_dark).unwrap().dark);
        assert!(!theme(&appearance.theme_light).unwrap().dark);
    }
}
