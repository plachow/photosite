//! Where the application keeps its things.
//!
//! The one rule here worth anything: **paths must be overridable.** In v1 they
//! were hard-wired to `LOCALAPPDATA`, which meant nothing could be measured or
//! tried out except against real data. Here the whole tree can be redirected
//! with one switch or one environment variable, so a benchmark, a test and a
//! second half-finished library all run to the side and never touch what
//! somebody has open.

use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

/// Environment variable that redirects the whole tree into one folder.
pub const DATA_OVERRIDE: &str = "PHOTOSITE_DATA";

/// Where everything lives. By platform convention out of the box; once
/// overridden, all of it under a single root — that is portable mode, and it
/// is exactly what a test and a memory stick both want.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Paths {
    /// The catalogue and anything else nobody wants to lose.
    pub data: PathBuf,
    /// Settings.
    pub config: PathBuf,
    /// Thumbnails and the like. Deletable without consequence.
    pub cache: PathBuf,
    /// Run logs and crash reports.
    pub logs: PathBuf,
    /// Is this portable mode?
    pub portable: bool,
}

impl Paths {
    /// Order of precedence: an explicit root, then `PHOTOSITE_DATA`, then the
    /// platform's conventions.
    pub fn resolve(explicit: Option<&Path>) -> Result<Self> {
        if let Some(root) = explicit {
            return Ok(Self::portable(root));
        }

        if let Some(root) = std::env::var_os(DATA_OVERRIDE) {
            return Ok(Self::portable(Path::new(&root)));
        }

        let dirs = directories::ProjectDirs::from("cz", "PhotoSite", "PhotoSite")
            .context("the system cannot say where application data belongs")?;
        Ok(Self {
            data: dirs.data_dir().to_path_buf(),
            config: dirs.config_dir().to_path_buf(),
            cache: dirs.cache_dir().to_path_buf(),
            logs: dirs.data_dir().join("logs"),
            portable: false,
        })
    }

    /// Everything under one root.
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

    /// Creates whatever is missing. Called once at startup.
    pub fn ensure(&self) -> Result<()> {
        for dir in [&self.data, &self.config, &self.cache, &self.logs] {
            std::fs::create_dir_all(dir)
                .with_context(|| format!("cannot create {}", dir.display()))?;
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_override_puts_everything_under_one_root() {
        let paths = Paths::portable(Path::new("/tmp/x"));
        assert!(paths.portable);
        assert!(paths.catalog().starts_with("/tmp/x"));
        assert!(paths.thumbnails().starts_with("/tmp/x"));
        assert!(paths.config_file().starts_with("/tmp/x"));
    }

    #[test]
    fn an_explicit_root_beats_the_environment() {
        // The variable is deliberately not set here, so the test does not
        // depend on the environment; the point is that an explicit path never
        // reads the environment at all.
        let paths = Paths::resolve(Some(Path::new("/tmp/y"))).unwrap();
        assert_eq!(paths, Paths::portable(Path::new("/tmp/y")));
    }

    #[test]
    fn ensure_builds_the_whole_tree() {
        let root = tempfile::tempdir().unwrap();
        let paths = Paths::portable(root.path());
        paths.ensure().unwrap();
        for dir in [&paths.data, &paths.config, &paths.cache, &paths.logs] {
            assert!(dir.is_dir(), "{} was not created", dir.display());
        }
    }
}
