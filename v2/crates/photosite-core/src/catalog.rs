//! The catalogue: SQLite and numbered migrations.
//!
//! Migrations are here from the very first table, because without them there
//! is no second release. Each is one piece of SQL and one step of
//! `user_version`; each runs in a transaction, so either all of it lands or
//! nothing does.

use crate::domain::{ColorLabel, FileIdentity, Flag, Organisation, Photo, PhotoId};
use anyhow::{Context, Result};
use rusqlite::{Connection, OptionalExtension as _, params};
use std::path::{Path, PathBuf};

struct Migration {
    name: &'static str,
    sql: &'static str,
}

/// Append **only at the end**. Never change an existing entry — on somebody
/// else's disk a database has already been built from it.
const MIGRATIONS: &[Migration] = &[
    Migration {
        name: "0001-photos-and-settings",
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
    },
    Migration {
        name: "0002-organisation",
        // What somebody says about a photograph, as against what was read out
        // of it. Every column has a default, so the rows already scanned are
        // correct the moment this runs rather than needing a pass of their
        // own.
        //
        // Keywords are a table and not a delimited column, which v1 had. Two
        // reasons: finding every photograph with a keyword becomes an index
        // lookup instead of a scan with LIKE over every row, and the set
        // cannot hold the same word twice. NOCASE on the keyword means
        // "Holiday" and "holiday" are one keyword — whichever was typed
        // first is the one kept, and nobody ends up with both in the filter
        // list.
        sql: "
        ALTER TABLE photos ADD COLUMN rating      INTEGER NOT NULL DEFAULT 0
                                                  CHECK (rating BETWEEN 0 AND 5);
        ALTER TABLE photos ADD COLUMN label       INTEGER NOT NULL DEFAULT 0;
        ALTER TABLE photos ADD COLUMN flag        INTEGER NOT NULL DEFAULT 0;
        ALTER TABLE photos ADD COLUMN title       TEXT;
        ALTER TABLE photos ADD COLUMN description TEXT;

        -- Has the file itself been read, or do we only know it is there?
        -- Opening a folder writes a row for every file at once, cheaply,
        -- so that a rating has somewhere to go before anything is decoded.
        -- The header is read afterwards on a thread, and this is how it
        -- knows what is left. Without it, a row that exists looks finished.
        ALTER TABLE photos ADD COLUMN indexed INTEGER NOT NULL DEFAULT 0;
        CREATE INDEX photos_indexed ON photos(indexed);

        CREATE INDEX photos_rating ON photos(rating);
        CREATE INDEX photos_label  ON photos(label);
        CREATE INDEX photos_flag   ON photos(flag);

        CREATE TABLE keywords (
            photo_id INTEGER NOT NULL REFERENCES photos(id) ON DELETE CASCADE,
            keyword  TEXT    NOT NULL COLLATE NOCASE,
            PRIMARY KEY (photo_id, keyword)
        );
        CREATE INDEX keywords_keyword ON keywords(keyword);
    ",
    },
    Migration {
        name: "0003-camera-and-lens",
        // What the filter offers has to be a column: "only the cameras that
        // are in this folder" means asking the catalogue, not opening seven
        // thousand files to find out.
        sql: "
        ALTER TABLE photos ADD COLUMN camera TEXT;
        ALTER TABLE photos ADD COLUMN lens   TEXT;

        CREATE INDEX photos_camera ON photos(camera);
        CREATE INDEX photos_lens   ON photos(lens);

        -- Every row was written before there were these columns, so none of
        -- them has been asked. Clearing the mark is what sends the
        -- background pass round again; it is cheap and it happens once.
        UPDATE photos SET indexed = 0;
    ",
    },
];

/// Which schema version this build is built for.
pub fn latest_version() -> i64 {
    MIGRATIONS.len() as i64
}

/// What separates one folder from the next on this system. A prefix match
/// needs it, or opening `/a/b` would also collect `/a/bc`.
const SEPARATOR: char = std::path::MAIN_SEPARATOR;

/// The columns [`read_photo`] expects, in its order. Written once so a column
/// added to one query and not the other cannot happen.
const COLUMNS: &str = "id, path, folder, file_size, modified_at, taken_at, width, height, \
                       orientation, camera, lens, rating, label, flag, title, description";

/// **Every column here is one the disk owns.** The organisation — rating,
/// label, flag, title, description — is deliberately absent from the update:
/// a rescan reads the file again and must not overwrite what a person said
/// about it. Touching a photograph on disk would otherwise clear its stars.
const UPSERT: &str = "
    INSERT INTO photos(path, folder, file_size, modified_at, taken_at, width, height,
                       orientation, camera, lens, indexed)
    VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, 1)
    ON CONFLICT(path) DO UPDATE SET
        folder = excluded.folder,
        file_size = excluded.file_size,
        modified_at = excluded.modified_at,
        taken_at = excluded.taken_at,
        width = excluded.width,
        height = excluded.height,
        orientation = excluded.orientation,
        camera = excluded.camera,
        lens = excluded.lens,
        indexed = 1
";

/// A row for a file we have only seen in a directory listing.
///
/// This is what makes a folder usable the instant it opens: every file gets
/// somewhere for a rating to go, at the cost of no file being opened. What
/// the header says arrives later, from [`Catalog::unindexed`] onwards.
///
/// `indexed` is cleared only when the file genuinely changed. Otherwise
/// opening a folder twice would order the whole library read again.
const UPSERT_IDENTITY: &str = "
    INSERT INTO photos(path, folder, file_size, modified_at, orientation, indexed)
    VALUES(?1, ?2, ?3, ?4, 1, 0)
    ON CONFLICT(path) DO UPDATE SET
        folder = excluded.folder,
        file_size = excluded.file_size,
        modified_at = excluded.modified_at,
        indexed = CASE
            WHEN photos.file_size = excluded.file_size
             AND photos.modified_at = excluded.modified_at THEN photos.indexed
            ELSE 0
        END
";

#[derive(Debug)]
pub struct Catalog {
    conn: Connection,
}

impl Catalog {
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("cannot create {}", parent.display()))?;
        }

        let conn = Connection::open(path)
            .with_context(|| format!("cannot open the catalogue {}", path.display()))?;
        Self::prepare(conn)
    }

    /// An in-memory catalogue. For tests, and for tests only.
    pub fn in_memory() -> Result<Self> {
        Self::prepare(Connection::open_in_memory()?)
    }

    fn prepare(conn: Connection) -> Result<Self> {
        // WAL so that reads do not block writes; NORMAL because the
        // catalogue can be rebuilt by a scan at any time and full fsync is
        // not worth it.
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
            "the catalogue is at schema {from}, this build handles at most {to} — \
             a newer PhotoSite has opened it"
        );

        for (at, migration) in MIGRATIONS.iter().enumerate().skip(from as usize) {
            let version = at as i64 + 1;
            tracing::info!(migration = migration.name, %version, "migrating the catalogue");
            let transaction = self.conn.transaction()?;
            transaction
                .execute_batch(migration.sql)
                .with_context(|| format!("migration {} failed", migration.name))?;
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

    /// Writes a photograph, or updates the one already at that path.
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
                photo.camera,
                photo.lens,
            ],
        )?;
        Ok(PhotoId(self.conn.query_row(
            "SELECT id FROM photos WHERE path = ?1",
            params![photo.path.to_string_lossy()],
            |row| row.get(0),
        )?))
    }

    /// Writes a batch in a single transaction.
    ///
    /// One row per transaction looks innocent, but it slows a folder scan by
    /// roughly twentyfold — SQLite has to settle up with the disk after every
    /// write. Hence batching from the start, not as a later optimisation.
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
                    photo.camera,
                    photo.lens,
                ])?;
            }
        }

        transaction.commit()?;
        Ok(())
    }

    /// Writes a row for every file, reading none of them.
    ///
    /// Batched, because a row per transaction slows a folder of seven
    /// thousand by roughly twentyfold — and this one runs while somebody is
    /// waiting to see the folder. Batched rather than done in a single
    /// transaction, because a whole library opened recursively is a hundred
    /// thousand rows and one transaction that long holds the lock for all of
    /// it.
    pub fn upsert_identities(&mut self, files: &[FileIdentity]) -> Result<()> {
        for chunk in files.chunks(2000) {
            let transaction = self.conn.transaction()?;
            {
                let mut statement = transaction.prepare(UPSERT_IDENTITY)?;
                for file in chunk {
                    let folder = file
                        .path
                        .parent()
                        .map(|p| p.to_string_lossy().into_owned())
                        .unwrap_or_default();
                    statement.execute(params![
                        file.path.to_string_lossy(),
                        folder,
                        file.file_size as i64,
                        file.modified_at,
                    ])?;
                }
            }

            transaction.commit()?;
        }

        Ok(())
    }

    /// The files in a folder whose header has not been read yet.
    ///
    /// What the background pass works through. When it comes back empty
    /// there is nothing left to learn about the folder.
    pub fn unindexed(&self, folder: &Path, recursive: bool) -> Result<Vec<PathBuf>> {
        let folder = folder.to_string_lossy().into_owned();
        let under = format!("{}{}", folder.trim_end_matches(SEPARATOR), SEPARATOR);
        let (predicate, args): (&str, Vec<&dyn rusqlite::ToSql>) = if recursive {
            (
                "(folder = ?1 OR substr(folder, 1, length(?2)) = ?2)",
                vec![&folder, &under],
            )
        } else {
            ("folder = ?1", vec![&folder])
        };

        let mut statement = self.conn.prepare(&format!(
            "SELECT path FROM photos WHERE indexed = 0 AND {predicate} ORDER BY path"
        ))?;
        Ok(statement
            .query_map(args.as_slice(), |row| row.get::<_, String>(0))?
            .map(|row| row.map(PathBuf::from))
            .collect::<rusqlite::Result<Vec<_>>>()?)
    }

    /// Paths the catalogue knows and that are unchanged — useful for an
    /// incremental scan, so it need not ask about every file one at a time.
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
        let found = self
            .conn
            .query_row(
                &format!("SELECT {COLUMNS} FROM photos WHERE path = ?1"),
                params![path.to_string_lossy()],
                read_photo,
            )
            .optional()?;
        match found {
            Some(mut photo) => {
                photo.organisation.keywords = self.keywords_of(photo.id)?;
                Ok(Some(photo))
            }
            None => Ok(None),
        }
    }

    /// The photographs in one folder, ordered by path, keywords and all.
    ///
    /// With `recursive`, everything underneath it too. The prefix is matched
    /// with `substr` rather than `LIKE`, because a folder called `my_photos`
    /// contains an underscore and `LIKE` reads that as "any character" —
    /// which would quietly pull in `myXphotos` as well.
    ///
    /// The keywords come in **one query for the whole folder**, not one per
    /// photograph. Seven thousand round trips to fill in a word or two each
    /// is the difference between opening a folder and waiting for it.
    pub fn in_folder(&self, folder: &Path, recursive: bool) -> Result<Vec<Photo>> {
        let folder = folder.to_string_lossy().into_owned();
        let under = format!("{}{}", folder.trim_end_matches(SEPARATOR), SEPARATOR);
        let predicate = if recursive {
            "(folder = ?1 OR substr(folder, 1, length(?2)) = ?2)"
        } else {
            "folder = ?1"
        };
        // SQLite refuses a parameter the statement never mentions, so the
        // list is built to match the predicate rather than always holding
        // both.
        let args: Vec<&dyn rusqlite::ToSql> = if recursive {
            vec![&folder, &under]
        } else {
            vec![&folder]
        };

        let mut statement = self.conn.prepare(&format!(
            "SELECT {COLUMNS} FROM photos WHERE {predicate} ORDER BY path"
        ))?;
        let mut photos = statement
            .query_map(args.as_slice(), read_photo)?
            .collect::<rusqlite::Result<Vec<_>>>()?;

        let mut statement = self.conn.prepare(&format!(
            "SELECT k.photo_id, k.keyword FROM keywords k
             JOIN photos p ON p.id = k.photo_id
             WHERE {}",
            predicate.replace("folder", "p.folder")
        ))?;
        let mut by_photo: std::collections::HashMap<i64, Vec<String>> =
            std::collections::HashMap::new();
        let rows = statement.query_map(args.as_slice(), |row| {
            Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
        })?;
        for row in rows {
            let (id, keyword) = row?;
            by_photo.entry(id).or_default().push(keyword);
        }

        for photo in &mut photos {
            if let Some(mut keywords) = by_photo.remove(&photo.id.0) {
                keywords.sort_by_key(|keyword| keyword.to_lowercase());
                photo.organisation.keywords = keywords;
            }
        }

        Ok(photos)
    }

    /// Every keyword on one photograph, in order.
    pub fn keywords_of(&self, photo: PhotoId) -> Result<Vec<String>> {
        let mut statement = self
            .conn
            .prepare("SELECT keyword FROM keywords WHERE photo_id = ?1 ORDER BY keyword")?;
        Ok(statement
            .query_map(params![photo.0], |row| row.get(0))?
            .collect::<rusqlite::Result<Vec<String>>>()?)
    }

    /// Every keyword anywhere in a folder, each once, in order.
    ///
    /// This is what the filter panel offers: the words that are actually on
    /// these photographs, not a list of everything ever typed.
    pub fn keywords_in_folder(&self, folder: &Path) -> Result<Vec<String>> {
        let mut statement = self.conn.prepare(
            "SELECT DISTINCT k.keyword FROM keywords k
             JOIN photos p ON p.id = k.photo_id
             WHERE p.folder = ?1 ORDER BY k.keyword",
        )?;
        Ok(statement
            .query_map(params![folder.to_string_lossy()], |row| row.get(0))?
            .collect::<rusqlite::Result<Vec<String>>>()?)
    }

    /// Sets the stars on a whole selection.
    ///
    /// Every one of these writes takes the selection rather than a single
    /// photograph, and does it in one transaction. Rating a hundred files one
    /// statement at a time is the same mistake as scanning one row at a time,
    /// and it shows in exactly the same way.
    pub fn set_rating(&mut self, photos: &[PhotoId], stars: u8) -> Result<()> {
        let stars = stars.min(Organisation::MAX_RATING) as i64;
        self.write_each(photos, "UPDATE photos SET rating = ?2 WHERE id = ?1", stars)
    }

    pub fn set_label(&mut self, photos: &[PhotoId], label: ColorLabel) -> Result<()> {
        self.write_each(
            photos,
            "UPDATE photos SET label = ?2 WHERE id = ?1",
            label.as_i64(),
        )
    }

    pub fn set_flag(&mut self, photos: &[PhotoId], flag: Flag) -> Result<()> {
        self.write_each(
            photos,
            "UPDATE photos SET flag = ?2 WHERE id = ?1",
            flag.as_i64(),
        )
    }

    /// One statement per photograph, all inside one transaction.
    fn write_each<T: rusqlite::ToSql>(
        &mut self,
        photos: &[PhotoId],
        sql: &str,
        value: T,
    ) -> Result<()> {
        if photos.is_empty() {
            return Ok(());
        }

        let transaction = self.conn.transaction()?;
        {
            let mut statement = transaction.prepare(sql)?;
            for photo in photos {
                statement.execute(params![photo.0, &value])?;
            }
        }

        transaction.commit()?;
        Ok(())
    }

    /// The title, or `None` to clear it. An empty string is not a title —
    /// it is somebody having deleted one, and storing it would leave the
    /// photograph looking titled.
    pub fn set_title(&self, photo: PhotoId, title: Option<&str>) -> Result<()> {
        self.conn.execute(
            "UPDATE photos SET title = ?2 WHERE id = ?1",
            params![photo.0, blank_to_none(title)],
        )?;
        Ok(())
    }

    pub fn set_description(&self, photo: PhotoId, description: Option<&str>) -> Result<()> {
        self.conn.execute(
            "UPDATE photos SET description = ?2 WHERE id = ?1",
            params![photo.0, blank_to_none(description)],
        )?;
        Ok(())
    }

    /// Adds keywords to a whole selection, leaving whatever is already there.
    ///
    /// Adding rather than replacing is the point: tagging fifty photographs
    /// "holiday" must not take away the names already on them.
    pub fn add_keywords<S: AsRef<str>>(
        &mut self,
        photos: &[PhotoId],
        keywords: &[S],
    ) -> Result<()> {
        let keywords = tidy_keywords(keywords);
        if photos.is_empty() || keywords.is_empty() {
            return Ok(());
        }

        let transaction = self.conn.transaction()?;
        {
            let mut statement = transaction
                .prepare("INSERT OR IGNORE INTO keywords(photo_id, keyword) VALUES(?1, ?2)")?;
            for photo in photos {
                for keyword in &keywords {
                    statement.execute(params![photo.0, keyword])?;
                }
            }
        }

        transaction.commit()?;
        Ok(())
    }

    pub fn remove_keywords<S: AsRef<str>>(
        &mut self,
        photos: &[PhotoId],
        keywords: &[S],
    ) -> Result<()> {
        let keywords = tidy_keywords(keywords);
        if photos.is_empty() || keywords.is_empty() {
            return Ok(());
        }

        let transaction = self.conn.transaction()?;
        {
            let mut statement =
                transaction.prepare("DELETE FROM keywords WHERE photo_id = ?1 AND keyword = ?2")?;
            for photo in photos {
                for keyword in &keywords {
                    statement.execute(params![photo.0, keyword])?;
                }
            }
        }

        transaction.commit()?;
        Ok(())
    }

    /// Replaces the keywords of one photograph with exactly this list. What
    /// the info panel does when somebody finishes editing the field.
    pub fn set_keywords<S: AsRef<str>>(&mut self, photo: PhotoId, keywords: &[S]) -> Result<()> {
        let keywords = tidy_keywords(keywords);
        let transaction = self.conn.transaction()?;
        transaction.execute("DELETE FROM keywords WHERE photo_id = ?1", params![photo.0])?;
        {
            let mut statement = transaction
                .prepare("INSERT OR IGNORE INTO keywords(photo_id, keyword) VALUES(?1, ?2)")?;
            for keyword in &keywords {
                statement.execute(params![photo.0, keyword])?;
            }
        }

        transaction.commit()?;
        Ok(())
    }

    pub fn count(&self) -> Result<i64> {
        Ok(self
            .conn
            .query_row("SELECT COUNT(*) FROM photos", [], |row| row.get(0))?)
    }

    /// Is the file in the catalogue and unchanged since?
    pub fn is_current(&self, identity: &FileIdentity) -> Result<bool> {
        Ok(self
            .by_path(&identity.path)?
            .map(|photo| identity.matches(&photo))
            .unwrap_or(false))
    }
}

/// What gets written into the catalogue. No `id`, because the database
/// hands that out.
#[derive(Debug, Clone, PartialEq)]
pub struct NewPhoto {
    pub path: PathBuf,
    pub file_size: u64,
    pub modified_at: i64,
    pub taken_at: Option<i64>,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub orientation: u8,
    pub camera: Option<String>,
    pub lens: Option<String>,
}

/// Builds a photograph out of one row of [`COLUMNS`].
///
/// Keywords are not here: they live in their own table, and asking for them
/// per row would be one query per tile. Whoever wants them fills them in
/// afterwards, for the whole folder at once.
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
        camera: row.get(9)?,
        lens: row.get(10)?,
        organisation: Organisation {
            rating: row
                .get::<_, i64>(11)?
                .clamp(0, Organisation::MAX_RATING as i64) as u8,
            label: ColorLabel::from_i64(row.get(12)?),
            flag: Flag::from_i64(row.get(13)?),
            title: row.get(14)?,
            description: row.get(15)?,
            keywords: Vec::new(),
        },
    })
}

/// An empty field is not a value, it is a cleared one.
///
/// Without this, deleting the text out of the title box leaves the
/// photograph holding an empty string — which reads as "titled" everywhere
/// that asks whether there is a title.
fn blank_to_none(text: Option<&str>) -> Option<String> {
    text.map(str::trim)
        .filter(|text| !text.is_empty())
        .map(str::to_owned)
}

/// Trims, drops the empty ones and removes repeats.
///
/// A keyword typed with a space around it is the same keyword; one typed
/// twice is one keyword. Doing this at the edge means nothing downstream has
/// to wonder.
fn tidy_keywords<S: AsRef<str>>(keywords: &[S]) -> Vec<String> {
    let mut tidy: Vec<String> = Vec::new();
    for keyword in keywords {
        let keyword = keyword.as_ref().trim();
        if keyword.is_empty() {
            continue;
        }

        if !tidy
            .iter()
            .any(|existing| existing.eq_ignore_ascii_case(keyword))
        {
            tidy.push(keyword.to_owned());
        }
    }

    tidy
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
            camera: Some("NIKON Z 6".to_owned()),
            lens: None,
        }
    }

    #[test]
    fn a_fresh_catalogue_is_at_the_latest_version() {
        let catalog = Catalog::in_memory().unwrap();
        assert_eq!(catalog.version().unwrap(), latest_version());
    }

    #[test]
    fn running_the_migrations_twice_spoils_nothing() {
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
    fn a_newer_schema_is_refused_rather_than_damaged() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("c.db");
        {
            let conn = Connection::open(&path).unwrap();
            conn.pragma_update(None, "user_version", latest_version() + 5)
                .unwrap();
        }

        let error = Catalog::open(&path).unwrap_err().to_string();
        assert!(error.contains("a newer PhotoSite"), "{error}");
    }

    #[test]
    fn upsert_overwrites_and_does_not_add_a_second_row() {
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
    fn listing_a_folder_returns_only_that_folder() {
        let catalog = Catalog::in_memory().unwrap();
        catalog.upsert(&sample("/a/one.jpg")).unwrap();
        catalog.upsert(&sample("/a/two.jpg")).unwrap();
        catalog.upsert(&sample("/b/three.jpg")).unwrap();
        let found = catalog.in_folder(Path::new("/a"), false).unwrap();
        assert_eq!(found.len(), 2);
        assert!(found.iter().all(|p| p.folder == Path::new("/a")));
    }

    #[test]
    fn an_unchanged_file_is_recognised_by_its_length_and_time() {
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
    fn settings_survive_a_rewrite() {
        let catalog = Catalog::in_memory().unwrap();
        assert_eq!(catalog.setting("x").unwrap(), None);
        catalog.set_setting("x", "1").unwrap();
        catalog.set_setting("x", "2").unwrap();
        assert_eq!(catalog.setting("x").unwrap().as_deref(), Some("2"));
    }

    /// The one that matters most in this whole file. Somebody rates a
    /// thousand photographs, an editor touches the files, the folder is
    /// scanned again — and the afternoon is gone.
    #[test]
    fn a_rescan_does_not_forget_the_stars() {
        let mut catalog = Catalog::in_memory().unwrap();
        let id = catalog.upsert(&sample("/a/b.jpg")).unwrap();
        catalog.set_rating(&[id], 4).unwrap();
        catalog.set_label(&[id], ColorLabel::Green).unwrap();
        catalog.set_flag(&[id], Flag::Picked).unwrap();
        catalog.set_title(id, Some("Sunrise")).unwrap();
        catalog.add_keywords(&[id], &["holiday"]).unwrap();

        // The file changed on disk and is read again, exactly as a rescan
        // would.
        let mut touched = sample("/a/b.jpg");
        touched.file_size = 4321;
        touched.modified_at = 1_800_000_000;
        catalog.upsert(&touched).unwrap();

        let photo = catalog.by_path(Path::new("/a/b.jpg")).unwrap().unwrap();
        assert_eq!(photo.file_size, 4321, "the disk still wins for its own");
        assert_eq!(photo.organisation.rating, 4);
        assert_eq!(photo.organisation.label, ColorLabel::Green);
        assert_eq!(photo.organisation.flag, Flag::Picked);
        assert_eq!(photo.organisation.title.as_deref(), Some("Sunrise"));
        assert_eq!(photo.organisation.keywords, ["holiday"]);
    }

    #[test]
    fn a_fresh_photograph_starts_with_nothing_said_about_it() {
        let catalog = Catalog::in_memory().unwrap();
        catalog.upsert(&sample("/a/b.jpg")).unwrap();
        let photo = catalog.by_path(Path::new("/a/b.jpg")).unwrap().unwrap();
        assert!(photo.organisation.is_empty());
    }

    #[test]
    fn the_stars_go_on_the_whole_selection_at_once() {
        let mut catalog = Catalog::in_memory().unwrap();
        let ids: Vec<_> = ["/a/one.jpg", "/a/two.jpg", "/a/three.jpg"]
            .iter()
            .map(|path| catalog.upsert(&sample(path)).unwrap())
            .collect();
        catalog.set_rating(&ids[..2], 5).unwrap();

        let folder = catalog.in_folder(Path::new("/a"), false).unwrap();
        let rated = folder
            .iter()
            .filter(|photo| photo.organisation.rating == 5)
            .count();
        assert_eq!(rated, 2);
    }

    #[test]
    fn a_rating_above_five_is_brought_back_down() {
        let mut catalog = Catalog::in_memory().unwrap();
        let id = catalog.upsert(&sample("/a/b.jpg")).unwrap();
        catalog.set_rating(&[id], 200).unwrap();
        let photo = catalog.by_path(Path::new("/a/b.jpg")).unwrap().unwrap();
        assert_eq!(photo.organisation.rating, 5);
    }

    #[test]
    fn writing_to_an_empty_selection_is_not_an_error() {
        let mut catalog = Catalog::in_memory().unwrap();
        catalog.set_rating(&[], 3).unwrap();
        catalog.add_keywords::<&str>(&[], &["x"]).unwrap();
    }

    #[test]
    fn an_emptied_title_is_cleared_rather_than_stored_blank() {
        let catalog = Catalog::in_memory().unwrap();
        let id = catalog.upsert(&sample("/a/b.jpg")).unwrap();
        catalog.set_title(id, Some("Sunrise")).unwrap();
        catalog.set_title(id, Some("   ")).unwrap();
        let photo = catalog.by_path(Path::new("/a/b.jpg")).unwrap().unwrap();
        assert_eq!(photo.organisation.title, None);
    }

    #[test]
    fn keywords_are_added_and_do_not_replace_what_is_there() {
        let mut catalog = Catalog::in_memory().unwrap();
        let id = catalog.upsert(&sample("/a/b.jpg")).unwrap();
        catalog.add_keywords(&[id], &["Anna", "Prague"]).unwrap();
        catalog.add_keywords(&[id], &["holiday"]).unwrap();
        assert_eq!(
            catalog.keywords_of(id).unwrap(),
            ["Anna", "holiday", "Prague"]
        );
    }

    #[test]
    fn the_same_keyword_in_another_case_is_the_same_keyword() {
        let mut catalog = Catalog::in_memory().unwrap();
        let id = catalog.upsert(&sample("/a/b.jpg")).unwrap();
        catalog.add_keywords(&[id], &["Holiday"]).unwrap();
        catalog
            .add_keywords(&[id], &["holiday", "HOLIDAY"])
            .unwrap();
        assert_eq!(catalog.keywords_of(id).unwrap(), ["Holiday"]);
    }

    #[test]
    fn a_keyword_typed_with_spaces_or_twice_lands_once_and_trimmed() {
        let mut catalog = Catalog::in_memory().unwrap();
        let id = catalog.upsert(&sample("/a/b.jpg")).unwrap();
        catalog
            .add_keywords(&[id], &["  holiday  ", "holiday", "", "   "])
            .unwrap();
        assert_eq!(catalog.keywords_of(id).unwrap(), ["holiday"]);
    }

    #[test]
    fn removing_a_keyword_leaves_the_others_alone() {
        let mut catalog = Catalog::in_memory().unwrap();
        let id = catalog.upsert(&sample("/a/b.jpg")).unwrap();
        catalog
            .add_keywords(&[id], &["Anna", "Prague", "holiday"])
            .unwrap();
        catalog.remove_keywords(&[id], &["prague"]).unwrap();
        assert_eq!(catalog.keywords_of(id).unwrap(), ["Anna", "holiday"]);
    }

    #[test]
    fn setting_the_keywords_replaces_the_whole_list() {
        let mut catalog = Catalog::in_memory().unwrap();
        let id = catalog.upsert(&sample("/a/b.jpg")).unwrap();
        catalog.add_keywords(&[id], &["Anna", "Prague"]).unwrap();
        catalog.set_keywords(id, &["holiday"]).unwrap();
        assert_eq!(catalog.keywords_of(id).unwrap(), ["holiday"]);

        catalog.set_keywords::<&str>(id, &[]).unwrap();
        assert!(catalog.keywords_of(id).unwrap().is_empty());
    }

    #[test]
    fn a_folder_comes_back_with_its_keywords_already_on_it() {
        let mut catalog = Catalog::in_memory().unwrap();
        let one = catalog.upsert(&sample("/a/one.jpg")).unwrap();
        catalog.upsert(&sample("/a/two.jpg")).unwrap();
        catalog.upsert(&sample("/b/other.jpg")).unwrap();
        catalog.add_keywords(&[one], &["Anna", "Prague"]).unwrap();

        let folder = catalog.in_folder(Path::new("/a"), false).unwrap();
        assert_eq!(folder.len(), 2);
        assert_eq!(folder[0].organisation.keywords, ["Anna", "Prague"]);
        assert!(folder[1].organisation.keywords.is_empty());
    }

    fn identity(path: &str) -> FileIdentity {
        FileIdentity {
            path: PathBuf::from(path),
            file_size: 1234,
            modified_at: 1_700_000_000,
        }
    }

    /// The whole point of writing a row before opening the file: a folder is
    /// ratable the moment it appears, not once every header has been read.
    #[test]
    fn a_folder_can_be_rated_before_a_single_file_is_opened() {
        let mut catalog = Catalog::in_memory().unwrap();
        catalog.upsert_identities(&[identity("/a/b.jpg")]).unwrap();

        let photo = catalog.by_path(Path::new("/a/b.jpg")).unwrap().unwrap();
        catalog.set_rating(&[photo.id], 3).unwrap();
        assert_eq!(
            catalog
                .by_path(Path::new("/a/b.jpg"))
                .unwrap()
                .unwrap()
                .organisation
                .rating,
            3
        );
    }

    #[test]
    fn a_file_only_seen_is_still_waiting_to_be_read() {
        let mut catalog = Catalog::in_memory().unwrap();
        catalog.upsert_identities(&[identity("/a/b.jpg")]).unwrap();
        assert_eq!(
            catalog.unindexed(Path::new("/a"), false).unwrap(),
            [PathBuf::from("/a/b.jpg")]
        );

        catalog.upsert(&sample("/a/b.jpg")).unwrap();
        assert!(
            catalog
                .unindexed(Path::new("/a"), false)
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn opening_the_same_folder_twice_does_not_order_it_read_again() {
        let mut catalog = Catalog::in_memory().unwrap();
        catalog.upsert_identities(&[identity("/a/b.jpg")]).unwrap();
        catalog.upsert(&sample("/a/b.jpg")).unwrap();

        // Exactly what a second open does: the same files, unchanged.
        catalog.upsert_identities(&[identity("/a/b.jpg")]).unwrap();
        assert!(
            catalog
                .unindexed(Path::new("/a"), false)
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn a_file_that_really_changed_is_read_again() {
        let mut catalog = Catalog::in_memory().unwrap();
        catalog.upsert(&sample("/a/b.jpg")).unwrap();

        let mut edited = identity("/a/b.jpg");
        edited.modified_at += 1;
        catalog.upsert_identities(&[edited]).unwrap();
        assert_eq!(catalog.unindexed(Path::new("/a"), false).unwrap().len(), 1);
    }

    #[test]
    fn seeing_a_file_again_does_not_forget_the_stars_either() {
        let mut catalog = Catalog::in_memory().unwrap();
        catalog.upsert_identities(&[identity("/a/b.jpg")]).unwrap();
        let id = catalog.by_path(Path::new("/a/b.jpg")).unwrap().unwrap().id;
        catalog.set_rating(&[id], 5).unwrap();
        catalog.set_label(&[id], ColorLabel::Blue).unwrap();

        let mut edited = identity("/a/b.jpg");
        edited.file_size = 99;
        catalog.upsert_identities(&[edited]).unwrap();

        let photo = catalog.by_path(Path::new("/a/b.jpg")).unwrap().unwrap();
        assert_eq!(photo.organisation.rating, 5);
        assert_eq!(photo.organisation.label, ColorLabel::Blue);
    }

    #[test]
    fn a_recursive_listing_reaches_the_subfolders_and_stops_at_the_name() {
        let catalog = Catalog::in_memory().unwrap();
        let sep = SEPARATOR;
        catalog
            .upsert(&sample(&format!("{sep}a{sep}one.jpg")))
            .unwrap();
        catalog
            .upsert(&sample(&format!("{sep}a{sep}deeper{sep}two.jpg")))
            .unwrap();
        // A sibling whose name merely starts the same way. A prefix match
        // with LIKE would have collected this one too.
        catalog
            .upsert(&sample(&format!("{sep}a_side{sep}three.jpg")))
            .unwrap();

        let root = PathBuf::from(format!("{sep}a"));
        assert_eq!(catalog.in_folder(&root, false).unwrap().len(), 1);
        assert_eq!(catalog.in_folder(&root, true).unwrap().len(), 2);
    }

    #[test]
    fn the_folder_offers_only_the_keywords_that_are_on_it() {
        let mut catalog = Catalog::in_memory().unwrap();
        let here = catalog.upsert(&sample("/a/one.jpg")).unwrap();
        let elsewhere = catalog.upsert(&sample("/b/other.jpg")).unwrap();
        catalog.add_keywords(&[here], &["Prague"]).unwrap();
        catalog.add_keywords(&[elsewhere], &["Vienna"]).unwrap();

        assert_eq!(
            catalog.keywords_in_folder(Path::new("/a")).unwrap(),
            ["Prague"]
        );
    }

    /// A catalogue built by the previous release opens, keeps its rows, and
    /// comes out with the new columns at their defaults. Without this the
    /// second release costs everybody their catalogue.
    #[test]
    fn a_catalogue_from_the_first_release_migrates_and_keeps_its_rows() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("c.db");
        {
            // Migration 0001 alone, exactly as the first release left it.
            let conn = Connection::open(&path).unwrap();
            conn.execute_batch(MIGRATIONS[0].sql).unwrap();
            conn.pragma_update(None, "user_version", 1).unwrap();
            conn.execute(
                "INSERT INTO photos(path, folder, file_size, modified_at, orientation)
                 VALUES('/a/b.jpg', '/a', 10, 20, 1)",
                [],
            )
            .unwrap();
        }

        let catalog = Catalog::open(&path).unwrap();
        assert_eq!(catalog.version().unwrap(), latest_version());
        assert_eq!(catalog.count().unwrap(), 1);
        let photo = catalog.by_path(Path::new("/a/b.jpg")).unwrap().unwrap();
        assert!(photo.organisation.is_empty());
    }
}
