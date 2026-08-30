//! Nastavení.
//!
//! Čtyři zásady, které stojí za to dodržet od začátku, protože každá z nich se
//! zpětně zavádí draho:
//!
//! 1. **Výchozí hodnoty jsou v kódu, jednou.** `Default` je jediný zdroj
//!    pravdy. Kdo nic nenastavil, dostane to, co považujeme za správné — a
//!    když se to rozmyslíme, dostane to i on, aniž by musel cokoliv mazat.
//! 2. **Do souboru jde jen to, co se liší.** Uložit celý strom znamená
//!    zmrazit dnešní výchozí hodnoty u každého, kdo aplikaci jednou spustil.
//!    Řídký soubor je navíc čitelný a je z něj vidět, co si kdo přenastavil.
//! 3. **Všechno má cestu.** `gallery.tile_size` se dá přečíst i zapsat jako
//!    text, takže nad tím půjde jednou vygenerovat obrazovka nastavení, aniž
//!    by se pro každou položku psal kód.
//! 4. **Reset je první třída.** Jedna položka, celá skupina, nebo všechno.
//!    Když se dá bezpečně vrátit, člověk si troufne zkoušet.
//!
//! Hotové sady nastavení tu schválně **nejsou**. Byly, a byly předčasné: jedna
//! z nich doslova opisovala výchozí hodnoty, takže by při jejich zlepšení
//! tiše zůstala na starých, a u ostatních se nedalo poznat, jestli je někdo
//! bude chtít. Reset na výchozí stav pokrývá „vrať mi to rozumné" celý.
//! Až bude nastavení tolik, že kombinace začnou dávat smysl, budou to data
//! v souboru, ne konstanty v kódu.

use crate::paths::Paths;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Settings {
    pub window: Window,
    pub gallery: Gallery,
    pub loading: Loading,
    pub appearance: Appearance,
}

/// Stav okna. Že se aplikace otevře tam, kde ji člověk nechal, je jedna
/// z prvních věcí, kterých si všimne — a to i když si toho nevšimne.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Window {
    pub width: f64,
    pub height: f64,
    pub x: Option<f64>,
    pub y: Option<f64>,
    pub maximized: bool,
    pub tree_width: f64,
    pub preview_width: f64,
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

/// Jak vypadá mřížka. Nic z toho není v kódu jako konstanta.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Gallery {
    pub tile_size: f64,
    /// Mezera mezi dlaždicemi.
    pub gap: f64,
    /// Poměr výšky obrázkové plochy k šířce dlaždice.
    pub tile_aspect: f64,
    /// Výška proužku s názvem pod fotkou.
    pub caption_height: f64,
    /// Kolik místa nechá rám kolem fotky.
    pub tile_padding: f64,
    /// Kolik řádků nad a pod viewportem se načítá dopředu.
    pub prefetch_rows: i64,
    pub show_captions: bool,
    pub recursive: bool,
    pub last_folder: Option<String>,
}

impl Default for Gallery {
    fn default() -> Self {
        Self {
            tile_size: 220.0,
            gap: 10.0,
            tile_aspect: 0.72,
            caption_height: 22.0,
            tile_padding: 7.0,
            prefetch_rows: 3,
            show_captions: true,
            recursive: false,
            last_folder: None,
        }
    }
}

/// Načítání obrázků. Tyhle hodnoty rozhodují o plynulosti, takže musí jít
/// osahat bez překladu.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Loading {
    /// Delší hrana dlaždice v obrazových bodech.
    pub thumb_size: i64,
    /// Delší hrana plného náhledu.
    pub preview_size: i64,
    /// Kolik hotových obrázků se za snímek nahraje do GPU. Bez stropu by
    /// jedna dávka zasekla vlákno, které kreslí.
    pub uploads_per_frame: i64,
    /// Kolik textur se drží, než začnou vypadávat nejdéle nepoužité.
    pub texture_budget: i64,
    /// Dekódovacích vláken; `0` znamená podle počtu jader.
    pub worker_threads: i64,
    /// Použít náhled vložený v EXIFu, než se dekóduje ostrý. Vypnout se to dá
    /// hlavně proto, aby šlo změřit, o kolik pomáhá.
    pub use_embedded_thumbnails: bool,
    /// Pojistka: nejdelší pauza mezi snímky, když se nic neděje. O hotovou
    /// práci se dekódovací vlákna hlásí sama, takže tohle jen kryje případ,
    /// kdy by se probuzení ztratilo. Krátký interval znamená budit se pro nic.
    pub idle_repaint_ms: i64,
}

impl Default for Loading {
    fn default() -> Self {
        Self {
            thumb_size: 320,
            preview_size: 2560,
            uploads_per_frame: 32,
            texture_budget: 900,
            worker_threads: 0,
            use_embedded_thumbnails: true,
            idle_repaint_ms: 1000,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Appearance {
    /// Klíč motivu, nebo `automatic` podle systému.
    pub theme: String,
    /// Který motiv použít, když systém hlásí tmavý režim.
    pub theme_dark: String,
    /// A který, když hlásí světlý.
    pub theme_light: String,
    /// Jazyk rozhraní. Neznámý spadne na `en-US`.
    pub language: String,
    /// Zvětšení celého rozhraní.
    pub ui_scale: f64,
}

impl Default for Appearance {
    fn default() -> Self {
        Self {
            theme: "automatic".to_owned(),
            theme_dark: "dark".to_owned(),
            theme_light: "light".to_owned(),
            language: crate::i18n::FALLBACK.to_string(),
            ui_scale: 1.0,
        }
    }
}

// ------------------------------------------------------------------ soubor

impl Settings {
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
            Ok(settings) => settings,
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

    /// Uloží jen to, co se liší od výchozího stavu.
    ///
    /// Píše se přes dočasný soubor, aby pád uprostřed zápisu nenechal na disku
    /// půlku.
    pub fn save(&self, paths: &Paths) -> Result<()> {
        let file = paths.config_file();
        if let Some(parent) = file.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let sparse = self.overrides()?;
        let text = if sparse.is_empty() {
            String::from("# Prázdné: všechno je na výchozích hodnotách.\n")
        } else {
            toml::to_string_pretty(&sparse).context("nastavení nelze serializovat")?
        };

        let temporary = file.with_extension("toml.tmp");
        std::fs::write(&temporary, text)
            .with_context(|| format!("nelze zapsat {}", temporary.display()))?;
        std::fs::rename(&temporary, &file)
            .with_context(|| format!("nelze přejmenovat na {}", file.display()))?;
        Ok(())
    }

    /// Co je jinak než ve výchozím stavu. To, co se ukládá.
    pub fn overrides(&self) -> Result<toml::Table> {
        let mine = toml::Table::try_from(self)?;
        let default = toml::Table::try_from(Settings::default())?;
        Ok(diff(&mine, &default))
    }

    /// Hodnota na cestě `gallery.tile_size`. Pro obecnou obrazovku nastavení.
    pub fn get(&self, path: &str) -> Option<toml::Value> {
        let mut value = toml::Value::try_from(self).ok()?;
        for part in path.split('.') {
            value = value.as_table()?.get(part)?.clone();
        }

        Some(value)
    }

    /// Zapíše hodnotu na cestu. Neznámá cesta nebo špatný typ je chyba, ne
    /// tiché nic — jinak by obrazovka nastavení mlčky nefungovala.
    pub fn set(&mut self, path: &str, value: toml::Value) -> Result<()> {
        let mut root = toml::Value::try_from(&*self)?;
        {
            let mut cursor = &mut root;
            let parts: Vec<&str> = path.split('.').collect();
            let (last, prefix) = parts.split_last().context("prázdná cesta")?;
            for part in prefix {
                cursor = cursor
                    .as_table_mut()
                    .and_then(|table| table.get_mut(*part))
                    .with_context(|| format!("cesta {path} nikam nevede"))?;
            }

            let table = cursor
                .as_table_mut()
                .with_context(|| format!("cesta {path} nikam nevede"))?;
            anyhow::ensure!(table.contains_key(*last), "cesta {path} neexistuje");
            table.insert((*last).to_owned(), value);
        }

        *self = root
            .try_into()
            .with_context(|| format!("hodnota na {path} nesedí typem"))?;
        Ok(())
    }

    /// Vrátí jednu položku nebo celou skupinu na výchozí hodnotu.
    pub fn reset(&mut self, path: &str) -> Result<()> {
        let default = Settings::default();
        let value = default
            .get(path)
            .with_context(|| format!("cesta {path} neexistuje"))?;
        if self
            .get(path)
            .map(|current| current == value)
            .unwrap_or(false)
        {
            return Ok(());
        }

        // Skupinu nelze nastavit jedním zápisem, protože `set` čeká list.
        match value.as_table() {
            Some(table) => {
                for key in table.keys() {
                    self.reset(&format!("{path}.{key}"))?;
                }
            }
            None => self.set(path, value)?,
        }

        Ok(())
    }

    /// Všechno zpátky na výchozí, kromě věcí, které nejsou předvolbou —
    /// naposledy otevřené složky a polohy okna se resetem nemyslí.
    pub fn reset_all(&mut self) {
        let keep_folder = self.gallery.last_folder.clone();
        let window = self.window.clone();
        *self = Settings::default();
        self.gallery.last_folder = keep_folder;
        self.window = window;
    }
}

/// Rekurzivní rozdíl dvou tabulek; zůstane jen to, co se liší.
fn diff(mine: &toml::Table, default: &toml::Table) -> toml::Table {
    let mut out = toml::Table::new();
    for (key, value) in mine {
        match (value, default.get(key)) {
            (toml::Value::Table(mine), Some(toml::Value::Table(default))) => {
                let nested = diff(mine, default);
                if !nested.is_empty() {
                    out.insert(key.clone(), toml::Value::Table(nested));
                }
            }
            (value, Some(default)) if value == default => {}
            (value, _) => {
                out.insert(key.clone(), value.clone());
            }
        }
    }

    out
}

// -------------------------------------------------------------- popis polí

/// Jakého druhu je hodnota. Podle tohohle se jednou vykreslí ovládací prvek.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Kind {
    Bool,
    Float {
        min: f64,
        max: f64,
    },
    Int {
        min: i64,
        max: i64,
    },
    Text,
    /// Volba z několika možností; hodnoty jsou klíče, ne názvy.
    Choice(&'static [&'static str]),
    /// Není předvolba — stav, který si aplikace pamatuje sama.
    State,
}

/// Jedna položka nastavení tak, jak ji jednou uvidí uživatel.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Tunable {
    pub path: &'static str,
    /// Klíč do překladu. Ani tady nejsou texty.
    pub label_key: &'static str,
    pub kind: Kind,
}

pub const TUNABLES: &[Tunable] = &[
    Tunable {
        path: "window.width",
        label_key: "setting-window-width",
        kind: Kind::State,
    },
    Tunable {
        path: "window.height",
        label_key: "setting-window-height",
        kind: Kind::State,
    },
    Tunable {
        path: "window.x",
        label_key: "setting-window-x",
        kind: Kind::State,
    },
    Tunable {
        path: "window.y",
        label_key: "setting-window-y",
        kind: Kind::State,
    },
    Tunable {
        path: "window.maximized",
        label_key: "setting-window-maximized",
        kind: Kind::State,
    },
    Tunable {
        path: "window.tree_width",
        label_key: "setting-window-tree-width",
        kind: Kind::State,
    },
    Tunable {
        path: "window.preview_width",
        label_key: "setting-window-preview-width",
        kind: Kind::State,
    },
    Tunable {
        path: "gallery.tile_size",
        label_key: "setting-tile-size",
        kind: Kind::Float {
            min: 80.0,
            max: 640.0,
        },
    },
    Tunable {
        path: "gallery.gap",
        label_key: "setting-gap",
        kind: Kind::Float {
            min: 0.0,
            max: 48.0,
        },
    },
    Tunable {
        path: "gallery.tile_aspect",
        label_key: "setting-tile-aspect",
        kind: Kind::Float { min: 0.3, max: 2.0 },
    },
    Tunable {
        path: "gallery.caption_height",
        label_key: "setting-caption-height",
        kind: Kind::Float {
            min: 0.0,
            max: 64.0,
        },
    },
    Tunable {
        path: "gallery.tile_padding",
        label_key: "setting-tile-padding",
        kind: Kind::Float {
            min: 0.0,
            max: 32.0,
        },
    },
    Tunable {
        path: "gallery.prefetch_rows",
        label_key: "setting-prefetch-rows",
        kind: Kind::Int { min: 0, max: 32 },
    },
    Tunable {
        path: "gallery.show_captions",
        label_key: "setting-show-captions",
        kind: Kind::Bool,
    },
    Tunable {
        path: "gallery.recursive",
        label_key: "setting-recursive",
        kind: Kind::Bool,
    },
    Tunable {
        path: "gallery.last_folder",
        label_key: "setting-last-folder",
        kind: Kind::State,
    },
    Tunable {
        path: "loading.thumb_size",
        label_key: "setting-thumb-size",
        kind: Kind::Int { min: 96, max: 1024 },
    },
    Tunable {
        path: "loading.preview_size",
        label_key: "setting-preview-size",
        kind: Kind::Int {
            min: 512,
            max: 8192,
        },
    },
    Tunable {
        path: "loading.uploads_per_frame",
        label_key: "setting-uploads-per-frame",
        kind: Kind::Int { min: 1, max: 256 },
    },
    Tunable {
        path: "loading.texture_budget",
        label_key: "setting-texture-budget",
        kind: Kind::Int { min: 64, max: 8192 },
    },
    Tunable {
        path: "loading.worker_threads",
        label_key: "setting-worker-threads",
        kind: Kind::Int { min: 0, max: 128 },
    },
    Tunable {
        path: "loading.use_embedded_thumbnails",
        label_key: "setting-use-embedded",
        kind: Kind::Bool,
    },
    Tunable {
        path: "loading.idle_repaint_ms",
        label_key: "setting-idle-repaint",
        kind: Kind::Int {
            min: 50,
            max: 10_000,
        },
    },
    Tunable {
        path: "appearance.theme",
        label_key: "setting-theme",
        kind: Kind::Text,
    },
    Tunable {
        path: "appearance.theme_dark",
        label_key: "setting-theme-dark",
        kind: Kind::Text,
    },
    Tunable {
        path: "appearance.theme_light",
        label_key: "setting-theme-light",
        kind: Kind::Text,
    },
    Tunable {
        path: "appearance.language",
        label_key: "setting-language",
        kind: Kind::Text,
    },
    Tunable {
        path: "appearance.ui_scale",
        label_key: "setting-ui-scale",
        kind: Kind::Float { min: 0.5, max: 3.0 },
    },
];

/// Všechny cesty, které v nastavení opravdu existují.
pub fn paths_in_settings() -> Vec<String> {
    fn walk(prefix: &str, table: &toml::Table, into: &mut Vec<String>) {
        for (key, value) in table {
            let path = if prefix.is_empty() {
                key.clone()
            } else {
                format!("{prefix}.{key}")
            };
            match value {
                toml::Value::Table(nested) => walk(&path, nested, into),
                _ => into.push(path),
            }
        }
    }

    let mut found = Vec::new();
    // `Option::None` se do TOML neserializuje, takže se přidá zvlášť.
    let mut probe = Settings::default();
    probe.window.x = Some(0.0);
    probe.window.y = Some(0.0);
    probe.gallery.last_folder = Some(String::new());
    if let Ok(table) = toml::Table::try_from(&probe) {
        walk("", &table, &mut found);
    }

    found.sort();
    found
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
        assert_eq!(Settings::load(&paths), Settings::default());
    }

    #[test]
    fn uklada_se_jen_to_co_je_jinak() {
        let (_dir, paths) = scratch();
        let mut settings = Settings::default();
        settings.save(&paths).unwrap();
        let text = std::fs::read_to_string(paths.config_file()).unwrap();
        assert!(
            !text.contains("tile_size"),
            "výchozí stav se nemá ukládat:\n{text}"
        );

        settings.gallery.tile_size = 333.0;
        settings.save(&paths).unwrap();
        let text = std::fs::read_to_string(paths.config_file()).unwrap();
        assert!(text.contains("tile_size"), "{text}");
        assert!(
            !text.contains("preview_size"),
            "netknuté se ukládat nemá:\n{text}"
        );
    }

    #[test]
    fn zmena_vychozi_hodnoty_dorazi_i_ke_stavajicimu_uzivateli() {
        // Když se do souboru ukládá jen rozdíl, nová výchozí hodnota se
        // projeví i tomu, kdo aplikaci už spustil. To je celý smysl.
        let (_dir, paths) = scratch();
        let mut settings = Settings::default();
        settings.gallery.tile_size = 333.0;
        settings.save(&paths).unwrap();

        let loaded = Settings::load(&paths);
        assert_eq!(loaded.gallery.tile_size, 333.0);
        assert_eq!(
            loaded.loading.texture_budget,
            Loading::default().texture_budget
        );
    }

    #[test]
    fn ulozene_se_nacte_zpatky_stejne() {
        let (_dir, paths) = scratch();
        let mut settings = Settings::default();
        settings.gallery.tile_size = 333.0;
        settings.window.maximized = true;
        settings.appearance.theme = "sepia".to_owned();
        settings.loading.worker_threads = 4;
        settings.save(&paths).unwrap();
        assert_eq!(Settings::load(&paths), settings);
    }

    #[test]
    fn poskozene_nastaveni_se_odlozi_a_neztrati() {
        let (_dir, paths) = scratch();
        std::fs::write(paths.config_file(), "tohle = není { toml").unwrap();
        assert_eq!(Settings::load(&paths), Settings::default());
        assert!(paths.config_file().with_extension("toml.broken").exists());
    }

    #[test]
    fn cestou_lze_cist_i_psat() {
        let mut settings = Settings::default();
        assert_eq!(
            settings.get("gallery.tile_size"),
            Some(toml::Value::Float(220.0))
        );
        settings
            .set("gallery.tile_size", toml::Value::Float(180.0))
            .unwrap();
        assert_eq!(settings.gallery.tile_size, 180.0);

        settings
            .set("gallery.show_captions", toml::Value::Boolean(false))
            .unwrap();
        assert!(!settings.gallery.show_captions);
    }

    #[test]
    fn neznama_cesta_nebo_spatny_typ_je_chyba() {
        let mut settings = Settings::default();
        assert!(
            settings
                .set("gallery.neexistuje", toml::Value::Integer(1))
                .is_err()
        );
        assert!(
            settings
                .set("nic.tam.neni", toml::Value::Integer(1))
                .is_err()
        );
        assert!(
            settings
                .set("gallery.tile_size", toml::Value::String("velké".into()))
                .is_err(),
            "špatný typ musí selhat, ne projít"
        );
    }

    #[test]
    fn reset_vraci_polozku_i_skupinu() {
        let mut settings = Settings::default();
        settings.gallery.tile_size = 999.0;
        settings.reset("gallery.tile_size").unwrap();
        assert_eq!(settings.gallery.tile_size, Gallery::default().tile_size);

        settings.loading.texture_budget = 1;
        settings.loading.thumb_size = 111;
        settings.reset("loading").unwrap();
        assert_eq!(settings.loading, Loading::default());
    }

    #[test]
    fn reset_vseho_nechá_stav_okna_a_slozku() {
        let mut settings = Settings::default();
        settings.gallery.tile_size = 999.0;
        settings.gallery.last_folder = Some("E:/fotky".to_owned());
        settings.window.width = 1234.0;
        settings.reset_all();

        assert_eq!(settings.gallery.tile_size, Gallery::default().tile_size);
        assert_eq!(settings.gallery.last_folder.as_deref(), Some("E:/fotky"));
        assert_eq!(settings.window.width, 1234.0, "polohu okna reset nemaže");
    }

    #[test]
    fn popis_poli_pokryva_presne_to_co_v_nastaveni_je() {
        let skutecne = paths_in_settings();
        let popsane: Vec<String> = TUNABLES
            .iter()
            .map(|t| t.path.to_owned())
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .collect();

        let chybi: Vec<&String> = skutecne
            .iter()
            .filter(|path| !popsane.contains(path))
            .collect();
        assert!(
            chybi.is_empty(),
            "v nastavení jsou pole bez popisu: {chybi:?}"
        );

        let navic: Vec<&String> = popsane
            .iter()
            .filter(|path| !skutecne.contains(path))
            .collect();
        assert!(
            navic.is_empty(),
            "popis odkazuje na neexistující pole: {navic:?}"
        );
    }

    #[test]
    fn kazde_pole_ma_preklad_popisku() {
        for tunable in TUNABLES {
            assert!(
                crate::i18n::has(tunable.label_key),
                "{} odkazuje na chybějící klíč {}",
                tunable.path,
                tunable.label_key
            );
        }
    }
}
