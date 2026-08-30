//! Katalog: SQLite a číslované migrace.
//!
//! Migrace jsou tu od první tabulky, protože bez nich se nedá vydat druhá
//! verze. Každá je jedno SQL a jeden krok `user_version`; běží v transakci,
//! takže buď projde celá, nebo se nestane nic.

use crate::domain::{FileIdentity, Photo, PhotoId};
use anyhow::{Context, Result};
use rusqlite::{Connection, OptionalExtension as _, params};
use std::path::{Path, PathBuf};

struct Migration {
    name: &'static str,
    sql: &'static str,
}

/// Přidávat **jen na konec**. Existující položku už nikdy neměnit — na
/// cizích discích je podle ní postavená databáze.
const MIGRATIONS: &[Migration] = &[Migration {
    name: "0001-fotky-a-nastaveni",
    sql: "
        CREATE TABLE photos (
            id           INTEGER PRIMARY KEY,
            path         TEXT    NOT NULL UNIQUE,
            folder       TEXT    NOT NULL,
            file_size    INTEGER NOT NULL,
            modified_at  INTEGER NOT NULL,
            taken_at     INTEGER,
            width        INTEGER,
            height       INTEGER,
            orientation  INTEGER NOT NULL DEFAULT 1
        );
        CREATE INDEX photos_folder ON photos(folder);
        CREATE INDEX photos_taken  ON photos(taken_at);

        CREATE TABLE settings (
            key   TEXT PRIMARY KEY,
            value TEXT NOT NULL
        );
    ",
}];

/// Na kolikátou verzi schématu je tenhle build stavěný.
pub fn latest_version() -> i64 {
    MIGRATIONS.len() as i64
}

const UPSERT: &str = "
    INSERT INTO photos(path, folder, file_size, modified_at, taken_at, width, height, orientation)
    VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
    ON CONFLICT(path) DO UPDATE SET
        folder = excluded.folder,
        file_size = excluded.file_size,
        modified_at = excluded.modified_at,
        taken_at = excluded.taken_at,
        width = excluded.width,
        height = excluded.height,
        orientation = excluded.orientation
";

#[derive(Debug)]
pub struct Catalog {
    conn: Connection,
}

impl Catalog {
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("nelze vytvořit {}", parent.display()))?;
        }

        let conn = Connection::open(path)
            .with_context(|| format!("nelze otevřít katalog {}", path.display()))?;
        Self::prepare(conn)
    }

    /// Katalog v paměti. Pro testy — a jen pro ně.
    pub fn in_memory() -> Result<Self> {
        Self::prepare(Connection::open_in_memory()?)
    }

    fn prepare(conn: Connection) -> Result<Self> {
        // WAL kvůli tomu, aby čtení neblokovalo zápis; NORMAL protože katalog
        // se dá kdykoliv obnovit skenem a plné fsync za to nestojí.
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "synchronous", "NORMAL")?;
        conn.pragma_update(None, "foreign_keys", true)?;
        let mut catalog = Self { conn };
        catalog.migrate()?;
        Ok(catalog)
    }

    pub fn version(&self) -> Result<i64> {
        Ok(self
            .conn
            .query_row("PRAGMA user_version", [], |row| row.get(0))?)
    }

    fn migrate(&mut self) -> Result<()> {
        let from = self.version()?;
        let to = latest_version();
        anyhow::ensure!(
            from <= to,
            "katalog je ze schématu {from}, tenhle build umí nejvýš {to} — \
             novější PhotoSite už ho otevřel"
        );

        for (at, migration) in MIGRATIONS.iter().enumerate().skip(from as usize) {
            let version = at as i64 + 1;
            tracing::info!(migrace = migration.name, %version, "migruji katalog");
            let transaction = self.conn.transaction()?;
            transaction
                .execute_batch(migration.sql)
                .with_context(|| format!("migrace {} selhala", migration.name))?;
            transaction.pragma_update(None, "user_version", version)?;
            transaction.commit()?;
        }

        Ok(())
    }

    pub fn setting(&self, key: &str) -> Result<Option<String>> {
        Ok(self
            .conn
            .query_row(
                "SELECT value FROM settings WHERE key = ?1",
                params![key],
                |row| row.get(0),
            )
            .optional()?)
    }

    pub fn set_setting(&self, key: &str, value: &str) -> Result<()> {
        self.conn.execute(
            "INSERT INTO settings(key, value) VALUES(?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![key, value],
        )?;
        Ok(())
    }

    /// Zapíše fotku, nebo aktualizuje tu, která už pod tou cestou je.
    pub fn upsert(&self, photo: &NewPhoto) -> Result<PhotoId> {
        let folder = photo
            .path
            .parent()
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_default();
        self.conn.execute(
            UPSERT,
            params![
                photo.path.to_string_lossy(),
                folder,
                photo.file_size as i64,
                photo.modified_at,
                photo.taken_at,
                photo.width,
                photo.height,
                photo.orientation,
            ],
        )?;
        Ok(PhotoId(self.conn.query_row(
            "SELECT id FROM photos WHERE path = ?1",
            params![photo.path.to_string_lossy()],
            |row| row.get(0),
        )?))
    }

    /// Zapíše dávku v jedné transakci.
    ///
    /// Jeden řádek na transakci vypadá nevinně, ale sken složky se tím
    /// zpomalí zhruba dvacetkrát — SQLite musí po každém zápisu srovnat účty
    /// s diskem. Dávka je tu proto od začátku, ne jako pozdější optimalizace.
    pub fn upsert_many(&mut self, photos: &[NewPhoto]) -> Result<()> {
        let transaction = self.conn.transaction()?;
        {
            let mut statement = transaction.prepare(UPSERT)?;
            for photo in photos {
                let folder = photo
                    .path
                    .parent()
                    .map(|p| p.to_string_lossy().into_owned())
                    .unwrap_or_default();
                statement.execute(params![
                    photo.path.to_string_lossy(),
                    folder,
                    photo.file_size as i64,
                    photo.modified_at,
                    photo.taken_at,
                    photo.width,
                    photo.height,
                    photo.orientation,
                ])?;
            }
        }

        transaction.commit()?;
        Ok(())
    }

    /// Cesty, které katalog zná a jsou beze změny — hodí se pro inkrementální
    /// sken, aby se nemusel ptát na každý soubor zvlášť.
    pub fn unchanged(
        &self,
        identities: &[FileIdentity],
    ) -> Result<std::collections::HashSet<PathBuf>> {
        let mut known = std::collections::HashSet::new();
        let mut statement = self
            .conn
            .prepare("SELECT file_size, modified_at FROM photos WHERE path = ?1")?;
        for identity in identities {
            let found: Option<(i64, i64)> = statement
                .query_row(params![identity.path.to_string_lossy()], |row| {
                    Ok((row.get(0)?, row.get(1)?))
                })
                .optional()?;
            if let Some((file_size, modified_at)) = found
                && file_size as u64 == identity.file_size
                && modified_at == identity.modified_at
            {
                known.insert(identity.path.clone());
            }
        }

        Ok(known)
    }

    pub fn by_path(&self, path: &Path) -> Result<Option<Photo>> {
        Ok(self
            .conn
            .query_row(
                "SELECT id, path, folder, file_size, modified_at, taken_at, width, height, orientation
                 FROM photos WHERE path = ?1",
                params![path.to_string_lossy()],
                read_photo,
            )
            .optional()?)
    }

    /// Fotky v jedné složce, seřazené podle cesty.
    pub fn in_folder(&self, folder: &Path) -> Result<Vec<Photo>> {
        let mut statement = self.conn.prepare(
            "SELECT id, path, folder, file_size, modified_at, taken_at, width, height, orientation
             FROM photos WHERE folder = ?1 ORDER BY path",
        )?;
        let rows = statement
            .query_map(params![folder.to_string_lossy()], read_photo)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    pub fn count(&self) -> Result<i64> {
        Ok(self
            .conn
            .query_row("SELECT COUNT(*) FROM photos", [], |row| row.get(0))?)
    }

    /// Je soubor v katalogu a nezměnil se od té doby?
    pub fn is_current(&self, identity: &FileIdentity) -> Result<bool> {
        Ok(self
            .by_path(&identity.path)?
            .map(|photo| identity.matches(&photo))
            .unwrap_or(false))
    }
}

/// Co se zapisuje do katalogu. Bez `id`, protože to přiděluje databáze.
#[derive(Debug, Clone, PartialEq)]
pub struct NewPhoto {
    pub path: PathBuf,
    pub file_size: u64,
    pub modified_at: i64,
    pub taken_at: Option<i64>,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub orientation: u8,
}

fn read_photo(row: &rusqlite::Row<'_>) -> rusqlite::Result<Photo> {
    let path: String = row.get(1)?;
    let folder: String = row.get(2)?;
    Ok(Photo {
        id: PhotoId(row.get(0)?),
        path: PathBuf::from(path),
        folder: PathBuf::from(folder),
        file_size: row.get::<_, i64>(3)? as u64,
        modified_at: row.get(4)?,
        taken_at: row.get(5)?,
        width: row.get(6)?,
        height: row.get(7)?,
        orientation: row.get(8)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(path: &str) -> NewPhoto {
        NewPhoto {
            path: PathBuf::from(path),
            file_size: 1234,
            modified_at: 1_700_000_000,
            taken_at: Some(1_600_000_000),
            width: Some(4000),
            height: Some(3000),
            orientation: 6,
        }
    }

    #[test]
    fn cerstvy_katalog_je_na_posledni_verzi() {
        let catalog = Catalog::in_memory().unwrap();
        assert_eq!(catalog.version().unwrap(), latest_version());
    }

    #[test]
    fn migrace_pustena_dvakrat_nic_nezkazi() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("c.db");
        {
            let catalog = Catalog::open(&path).unwrap();
            catalog.upsert(&sample("/a/b.jpg")).unwrap();
        }

        let catalog = Catalog::open(&path).unwrap();
        assert_eq!(catalog.version().unwrap(), latest_version());
        assert_eq!(catalog.count().unwrap(), 1);
    }

    #[test]
    fn novejsi_schema_se_odmitne_misto_poskozeni() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("c.db");
        {
            let conn = Connection::open(&path).unwrap();
            conn.pragma_update(None, "user_version", latest_version() + 5)
                .unwrap();
        }

        let error = Catalog::open(&path).unwrap_err().to_string();
        assert!(error.contains("novější PhotoSite"), "{error}");
    }

    #[test]
    fn upsert_prepise_a_nezaloz_druhy_radek() {
        let catalog = Catalog::in_memory().unwrap();
        let first = catalog.upsert(&sample("/a/b.jpg")).unwrap();
        let mut changed = sample("/a/b.jpg");
        changed.file_size = 9999;
        let second = catalog.upsert(&changed).unwrap();
        assert_eq!(first, second);
        assert_eq!(catalog.count().unwrap(), 1);
        assert_eq!(
            catalog
                .by_path(Path::new("/a/b.jpg"))
                .unwrap()
                .unwrap()
                .file_size,
            9999
        );
    }

    #[test]
    fn listovani_slozky_vraci_jen_ji() {
        let catalog = Catalog::in_memory().unwrap();
        catalog.upsert(&sample("/a/one.jpg")).unwrap();
        catalog.upsert(&sample("/a/two.jpg")).unwrap();
        catalog.upsert(&sample("/b/three.jpg")).unwrap();
        let found = catalog.in_folder(Path::new("/a")).unwrap();
        assert_eq!(found.len(), 2);
        assert!(found.iter().all(|p| p.folder == Path::new("/a")));
    }

    #[test]
    fn nezmeneny_soubor_se_pozna_podle_delky_a_casu() {
        let catalog = Catalog::in_memory().unwrap();
        catalog.upsert(&sample("/a/b.jpg")).unwrap();
        let same = FileIdentity {
            path: PathBuf::from("/a/b.jpg"),
            file_size: 1234,
            modified_at: 1_700_000_000,
        };
        assert!(catalog.is_current(&same).unwrap());

        let touched = FileIdentity {
            modified_at: 1_700_000_001,
            ..same
        };
        assert!(!catalog.is_current(&touched).unwrap());
    }

    #[test]
    fn nastaveni_prezije_prepis() {
        let catalog = Catalog::in_memory().unwrap();
        assert_eq!(catalog.setting("x").unwrap(), None);
        catalog.set_setting("x", "1").unwrap();
        catalog.set_setting("x", "2").unwrap();
        assert_eq!(catalog.setting("x").unwrap().as_deref(), Some("2"));
    }
}
