//! Kam aplikace ukládá svoje věci.
//!
//! Jediné pravidlo, které tu za něco stojí: **cesty musí jít přebít.** Ve v1
//! byly natvrdo na `LOCALAPPDATA` a znamenalo to, že cokoliv se dalo změřit
//! nebo vyzkoušet jen na ostrých datech. Tady se dá celý strom přesměrovat
//! jedním přepínačem nebo proměnnou prostředí, takže benchmark, test i druhá
//! rozdělaná knihovna běží stranou a nikdy si nesáhnou na to, co má člověk
//! rozdělané.

use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

/// Proměnná prostředí, která přesměruje celý strom do jedné složky.
pub const DATA_OVERRIDE: &str = "PHOTOSITE_DATA";

/// Kde co leží. Ve výchozím stavu podle zvyklostí platformy, po přebití
/// všechno pod jedním kořenem — tomu se říká přenosný režim a je to přesně to,
/// co chce test i flash disk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Paths {
    /// Katalog a další data, o která člověk nechce přijít.
    pub data: PathBuf,
    /// Nastavení.
    pub config: PathBuf,
    /// Náhledy a spol. Smazatelné bez následků.
    pub cache: PathBuf,
    /// Záznamy běhu a hlášení o pádech.
    pub logs: PathBuf,
    /// Je tohle přenosný režim?
    pub portable: bool,
}

impl Paths {
    /// Pořadí přednosti: výslovný kořen, pak `PHOTOSITE_DATA`, pak zvyklosti
    /// platformy.
    pub fn resolve(explicit: Option<&Path>) -> Result<Self> {
        if let Some(root) = explicit {
            return Ok(Self::portable(root));
        }

        if let Some(root) = std::env::var_os(DATA_OVERRIDE) {
            return Ok(Self::portable(Path::new(&root)));
        }

        let dirs = directories::ProjectDirs::from("cz", "PhotoSite", "PhotoSite")
            .context("systém neumí říct, kam patří data aplikace")?;
        Ok(Self {
            data: dirs.data_dir().to_path_buf(),
            config: dirs.config_dir().to_path_buf(),
            cache: dirs.cache_dir().to_path_buf(),
            logs: dirs.data_dir().join("logs"),
            portable: false,
        })
    }

    /// Všechno pod jedním kořenem.
    pub fn portable(root: &Path) -> Self {
        Self {
            data: root.join("data"),
            config: root.join("config"),
            cache: root.join("cache"),
            logs: root.join("logs"),
            portable: true,
        }
    }

    pub fn catalog(&self) -> PathBuf {
        self.data.join("catalog.db")
    }

    pub fn config_file(&self) -> PathBuf {
        self.config.join("photosite.toml")
    }

    pub fn thumbnails(&self) -> PathBuf {
        self.cache.join("thumbnails")
    }

    /// Vytvoří, co chybí. Volá se jednou při startu.
    pub fn ensure(&self) -> Result<()> {
        for dir in [&self.data, &self.config, &self.cache, &self.logs] {
            std::fs::create_dir_all(dir)
                .with_context(|| format!("nelze vytvořit {}", dir.display()))?;
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prebiti_slozi_vsechno_pod_jeden_koren() {
        let paths = Paths::portable(Path::new("/tmp/x"));
        assert!(paths.portable);
        assert!(paths.catalog().starts_with("/tmp/x"));
        assert!(paths.thumbnails().starts_with("/tmp/x"));
        assert!(paths.config_file().starts_with("/tmp/x"));
    }

    #[test]
    fn vyslovny_koren_ma_prednost_pred_promennou() {
        // Proměnná se tu nenastavuje, aby test nezávisel na prostředí; jde
        // o to, že výslovná cesta prostředí vůbec nečte.
        let paths = Paths::resolve(Some(Path::new("/tmp/y"))).unwrap();
        assert_eq!(paths, Paths::portable(Path::new("/tmp/y")));
    }

    #[test]
    fn ensure_vyrobi_cely_strom() {
        let root = tempfile::tempdir().unwrap();
        let paths = Paths::portable(root.path());
        paths.ensure().unwrap();
        for dir in [&paths.data, &paths.config, &paths.cache, &paths.logs] {
            assert!(dir.is_dir(), "{} nevznikla", dir.display());
        }
    }
}
