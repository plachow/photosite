//! Nastavení v jednom TOML souboru.
//!
//! Dvě zásady, obě z toho, jak se aplikace chovají špatně:
//!
//! * **Chybějící soubor není chyba.** První spuštění je normální stav.
//! * **Rozbitý soubor se nikdy nezahodí mlčky.** Odloží se stranou s příponou
//!   `.broken`, do logu jde proč, a jede se na výchozích hodnotách. Člověk,
//!   který si nastavení ručně upravil a udělal překlep, o něj nesmí přijít.

use crate::paths::Paths;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    pub window: Window,
    pub gallery: Gallery,
    pub appearance: Appearance,
}

/// Stav okna. Že se aplikace otevře tam, kde ji člověk nechal, je jedna
/// z prvních věcí, kterých si všimne — a to i když si toho nevšimne.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Window {
    pub width: f32,
    pub height: f32,
    pub x: Option<f32>,
    pub y: Option<f32>,
    pub maximized: bool,
    /// Šířky levého a pravého doku v pixelech.
    pub tree_width: f32,
    pub preview_width: f32,
}

impl Default for Window {
    fn default() -> Self {
        Self {
            width: 1600.0,
            height: 1000.0,
            x: None,
            y: None,
            maximized: false,
            tree_width: 250.0,
            preview_width: 620.0,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Gallery {
    pub tile_size: f32,
    pub recursive: bool,
    pub last_folder: Option<String>,
}

impl Default for Gallery {
    fn default() -> Self {
        Self {
            tile_size: 220.0,
            recursive: false,
            last_folder: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Appearance {
    /// Jméno palety. Neznámé jméno spadne na první paletu, ne na paniku.
    pub theme: String,
    /// Jazyk rozhraní. Neznámý spadne na `en-US`.
    pub language: String,
}

impl Default for Appearance {
    fn default() -> Self {
        Self {
            theme: "tmava".to_owned(),
            language: crate::i18n::FALLBACK.to_string(),
        }
    }
}

impl Config {
    /// Načte nastavení. Nikdy neselže kvůli obsahu souboru — nanejvýš se
    /// vrátí výchozí hodnoty a poškozený soubor se odloží stranou.
    pub fn load(paths: &Paths) -> Self {
        let file = paths.config_file();
        let text = match std::fs::read_to_string(&file) {
            Ok(text) => text,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                tracing::debug!(path = %file.display(), "nastavení zatím není, jedeme na výchozím");
                return Self::default();
            }
            Err(error) => {
                tracing::warn!(path = %file.display(), %error, "nastavení nelze přečíst");
                return Self::default();
            }
        };

        match toml::from_str(&text) {
            Ok(config) => config,
            Err(error) => {
                let broken = file.with_extension("toml.broken");
                tracing::error!(
                    path = %file.display(),
                    odlozeno = %broken.display(),
                    %error,
                    "nastavení je poškozené"
                );
                let _ = std::fs::rename(&file, &broken);
                Self::default()
            }
        }
    }

    /// Uloží nastavení. Píše se přes dočasný soubor, aby pád uprostřed zápisu
    /// nenechal na disku půlku.
    pub fn save(&self, paths: &Paths) -> Result<()> {
        let file = paths.config_file();
        if let Some(parent) = file.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let text = toml::to_string_pretty(self).context("nastavení nelze serializovat")?;
        let temporary = file.with_extension("toml.tmp");
        std::fs::write(&temporary, text)
            .with_context(|| format!("nelze zapsat {}", temporary.display()))?;
        std::fs::rename(&temporary, &file)
            .with_context(|| format!("nelze přejmenovat na {}", file.display()))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch() -> (tempfile::TempDir, Paths) {
        let dir = tempfile::tempdir().unwrap();
        let paths = Paths::portable(dir.path());
        paths.ensure().unwrap();
        (dir, paths)
    }

    #[test]
    fn chybejici_soubor_da_vychozi_hodnoty() {
        let (_dir, paths) = scratch();
        assert_eq!(Config::load(&paths), Config::default());
    }

    #[test]
    fn ulozene_se_nacte_zpatky_stejne() {
        let (_dir, paths) = scratch();
        let mut config = Config::default();
        config.gallery.tile_size = 333.0;
        config.window.maximized = true;
        config.appearance.theme = "sepie".to_owned();
        config.save(&paths).unwrap();
        assert_eq!(Config::load(&paths), config);
    }

    #[test]
    fn poskozene_nastaveni_se_odlozi_a_neztrati() {
        let (_dir, paths) = scratch();
        std::fs::write(paths.config_file(), "tohle = není { toml").unwrap();
        assert_eq!(Config::load(&paths), Config::default());
        assert!(
            paths.config_file().with_extension("toml.broken").exists(),
            "poškozené nastavení se musí odložit, ne zahodit"
        );
    }
}
