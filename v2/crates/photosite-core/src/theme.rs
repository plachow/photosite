//! Motivy jako data.
//!
//! Barvy nejsou konstanty v kódu, ale hodnoty, které jde serializovat. Dnes
//! jsou vestavěné, ale právě proto, že jsou to data, půjde je časem načíst ze
//! souboru, aniž by se čehokoliv dotklo v kódu. Kdyby byly zadrátované jako
//! `Color32::from_rgb(...)` uvnitř kreslení, znamenal by uživatelský motiv
//! přepsat každé místo, kde se něco kreslí.
//!
//! Jádro o žádné grafické knihovně neví — [`Color`] je trojice bajtů, ne typ
//! z egui. Převod si dělá vrstva UI.

use crate::settings::Appearance;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// Barva v podobě, kterou člověk přečte i napíše: `#2A2A2C`.
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
}

impl Serialize for Color {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_hex())
    }
}

impl<'de> Deserialize<'de> for Color {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        Color::parse(&text)
            .ok_or_else(|| serde::de::Error::custom(format!("{text:?} není barva jako #RRGGBB")))
    }
}

/// Sada barev jednoho motivu.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Palette {
    /// Pozadí za vším.
    pub window: Color,
    /// Doky.
    pub panel: Color,
    /// Rám diapozitivu.
    pub tile: Color,
    /// Plocha pod fotkou. Tmavší než rám, aby fotka „seděla v okně".
    pub well: Color,
    /// Proužek s názvem.
    pub caption: Color,
    pub text: Color,
    pub dim: Color,
    pub accent: Color,
    /// Horní a levá hrana rámu. Jen náznak, ne vypouklé tlačítko.
    pub bevel_light: Color,
    pub bevel_dark: Color,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Theme {
    /// Klíč do nastavení. Nikdy se nemění.
    pub id: &'static str,
    /// Klíč do překladu. Ani tady nejsou texty.
    pub label_key: &'static str,
    /// Je to tmavý motiv? Podle tohohle se řídí automatická volba.
    pub dark: bool,
    pub palette: Palette,
}

/// Hodnota `appearance.theme`, která znamená „podle systému".
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
            dim: Color::hex(0x8A_8A90),
            accent: Color::hex(0x5B_9DD9),
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
            accent: Color::hex(0x1F_6FB2),
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
            dim: Color::hex(0xA6_A6AA),
            accent: Color::hex(0x6F_B0E8),
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
            dim: Color::hex(0x99_8C77),
            accent: Color::hex(0xC9_944F),
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
            dim: Color::hex(0x7E_94A6),
            accent: Color::hex(0x4F_C3D9),
            bevel_light: Color::hex(0x3C_5064),
            bevel_dark: Color::hex(0x0C_1218),
        },
    },
];

pub fn theme(id: &str) -> Option<&'static Theme> {
    THEMES.iter().find(|theme| theme.id == id)
}

/// Který motiv se má použít.
///
/// `system_dark` je to, co hlásí systém — `None` znamená, že se ho nepodařilo
/// zeptat. Neznámé jméno motivu spadne na první v seznamu, ne na paniku;
/// překlep v nastavení nesmí aplikaci připravit o barvy.
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

    #[test]
    fn barva_tam_a_zpatky() {
        for text in ["#000000", "#FFFFFF", "#2A2A2C", "#4FC3D9"] {
            assert_eq!(Color::parse(text).unwrap().to_hex(), text);
        }

        assert_eq!(Color::parse("2A2A2C"), Color::parse("#2A2A2C"));
        assert_eq!(Color::parse("#ZZZZZZ"), None);
        assert_eq!(Color::parse("#FFF"), None);
    }

    #[test]
    fn motiv_se_da_ulozit_a_nacist() {
        // Tohle je celý důvod, proč jsou barvy data: uživatelský motiv bude
        // jen tenhle TOML v souboru.
        let text = toml::to_string(&THEMES[0].palette).unwrap();
        assert!(text.contains("#22"), "{text}");
        let zpatky: Palette = toml::from_str(&text).unwrap();
        assert_eq!(zpatky, THEMES[0].palette);
    }

    #[test]
    fn identifikatory_motivu_jsou_jedinecne() {
        let mut ids: Vec<_> = THEMES.iter().map(|theme| theme.id).collect();
        let count = ids.len();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), count);
    }

    #[test]
    fn kazdy_motiv_ma_preklad() {
        for theme in THEMES {
            assert!(crate::i18n::has(theme.label_key), "{}", theme.id);
        }
    }

    #[test]
    fn automaticky_motiv_jde_podle_systemu() {
        let appearance = Appearance::default();
        assert_eq!(appearance.theme, AUTOMATIC);
        assert_eq!(resolve(&appearance, Some(true)).id, "dark");
        assert_eq!(resolve(&appearance, Some(false)).id, "light");
        // Když se systému nedá zeptat, tmavý je pro fotky bezpečnější volba.
        assert!(resolve(&appearance, None).dark);
    }

    #[test]
    fn vyslovny_motiv_prebije_automatiku() {
        let appearance = Appearance {
            theme: "sepia".to_owned(),
            ..Appearance::default()
        };
        assert_eq!(resolve(&appearance, Some(false)).id, "sepia");
    }

    #[test]
    fn preklep_v_nastaveni_neshodi_barvy() {
        let appearance = Appearance {
            theme: "neexistuje".to_owned(),
            ..Appearance::default()
        };
        assert_eq!(resolve(&appearance, Some(true)).id, THEMES[0].id);
    }

    #[test]
    fn vychozi_motivy_pro_automatiku_existuji() {
        let appearance = Appearance::default();
        assert!(theme(&appearance.theme_dark).is_some());
        assert!(theme(&appearance.theme_light).is_some());
        assert!(theme(&appearance.theme_dark).unwrap().dark);
        assert!(!theme(&appearance.theme_light).unwrap().dark);
    }
}
