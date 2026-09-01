//! The catalogue: SQLite and numbered migrations.
//!
//! Migrations are here from the very first table, because without them there
//! is no second release. Each is one piece of SQL and one step of
//! `user_version`; each runs in a transaction, so either all of it lands or
//! nothing does.

use crate::domain::{ColorLabel, FileIdentity, Flag, Organisation, Photo, PhotoId};
use crate::place::{Place, Reason, Verdict};
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
    Migration {
        name: "0004-metadata-outbox",
        // What still has to be written into the files themselves.
        //
        // There is no payload here, and that is the point. v1 queued the
        // change; this queues the *photograph*, and what gets written is
        // whatever the catalogue says at the moment the write happens. A
        // retry therefore writes the truth as it stands rather than a change
        // that may since have been undone, rating a photograph twice leaves
        // one entry rather than two, and there is no way for the queue and
        // the catalogue to disagree.
        //
        // One row per photograph, so the primary key does the collapsing.
        sql: "
        CREATE TABLE metadata_outbox (
            photo_id   INTEGER PRIMARY KEY REFERENCES photos(id) ON DELETE CASCADE,
            not_before INTEGER NOT NULL,
            attempts   INTEGER NOT NULL DEFAULT 0,
            last_error TEXT
        );
        CREATE INDEX metadata_outbox_due ON metadata_outbox(not_before);
    ",
    },
    Migration {
        name: "0005-where-it-was-taken",
        // The position and what we make of it. The verdict is stored rather
        // than worked out on every tile because the evidence it rests on —
        // the error estimate, the method, the age of the fix — is in the
        // file and not in the catalogue, and re-reading a file to draw a
        // badge is a file open per photograph per frame.
        sql: "
        ALTER TABLE photos ADD COLUMN latitude  REAL;
        ALTER TABLE photos ADD COLUMN longitude REAL;
        ALTER TABLE photos ADD COLUMN place_verdict INTEGER NOT NULL DEFAULT 0;
        ALTER TABLE photos ADD COLUMN place_reason  INTEGER NOT NULL DEFAULT 0;
    ",
    },
    Migration {
        name: "0006-faces-and-people",
        // Who is in the photograph.
        //
        // A face hangs off the photograph's **number**, not off its path.
        // v1 keyed its face rows by path, so renaming a file orphaned every
        // face on it and the next sweep found them all again as strangers.
        // Here the row travels with the photograph exactly as its stars do,
        // and a deleted photograph takes its faces with it because the
        // foreign key says so rather than because somebody remembered.
        //
        // The embedding is a blob and not a column per dimension: it is
        // never queried on, only fetched whole and compared in memory. A
        // hundred and twenty-eight REAL columns would be a table nobody
        // could read and a schema change on the day the model changes.
        sql: "
        CREATE TABLE people (
            id   INTEGER PRIMARY KEY,
            name TEXT NOT NULL UNIQUE COLLATE NOCASE
        );

        CREATE TABLE faces (
            id         INTEGER PRIMARY KEY,
            photo_id   INTEGER NOT NULL REFERENCES photos(id) ON DELETE CASCADE,
            -- Fractions of the frame, so the rectangle means the same on a
            -- tile, in the preview and in the file.
            x          REAL NOT NULL,
            y          REAL NOT NULL,
            w          REAL NOT NULL,
            h          REAL NOT NULL,
            confidence REAL NOT NULL,
            embedding  BLOB NOT NULL,
            person_id           INTEGER REFERENCES people(id) ON DELETE SET NULL,
            -- Probably this person. Nothing has been written anywhere on the
            -- strength of it, and nothing will be until somebody answers.
            suggested_person_id INTEGER REFERENCES people(id) ON DELETE SET NULL,
            -- NULL is not a low score: it is a face the expression models
            -- have never seen, and it is what the scoring pass looks for.
            smile      REAL,
            eyes_open  REAL,
            -- Waved away as a stranger. The row stays so the photograph
            -- still counts as scanned.
            ignored    INTEGER NOT NULL DEFAULT 0
        );
        CREATE INDEX faces_photo     ON faces(photo_id);
        CREATE INDEX faces_person    ON faces(person_id);
        CREATE INDEX faces_suggested ON faces(suggested_person_id);

        -- Somebody on the photograph with no face to frame: turned away,
        -- behind the camera, in the dark. A badge and a filter mean the same
        -- thing whichever of the two put the name there.
        CREATE TABLE photo_people (
            photo_id  INTEGER NOT NULL REFERENCES photos(id) ON DELETE CASCADE,
            person_id INTEGER NOT NULL REFERENCES people(id) ON DELETE CASCADE,
            PRIMARY KEY (photo_id, person_id)
        );

        -- What the file looked like when it was last swept, so an
        -- interrupted sweep resumes and an unchanged file is skipped.
        CREATE TABLE face_scans (
            photo_id    INTEGER PRIMARY KEY REFERENCES photos(id) ON DELETE CASCADE,
            file_size   INTEGER NOT NULL,
            modified_at INTEGER NOT NULL,
            faces       INTEGER NOT NULL,
            scanned_at  INTEGER NOT NULL
        );
    ",
    },
];

/// How many times a file is tried before it is left alone.
///
/// It stays in the queue with its error afterwards rather than being thrown
/// away: a photograph on a disconnected drive should be written when the
/// drive comes back, and one that can never be written is something somebody
/// needs to be told about.
pub const MAX_ATTEMPTS: i64 = 8;

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
                       orientation, camera, lens, rating, label, flag, title, description, \
                       latitude, longitude, place_verdict, place_reason";

/// The same list, each column under a table alias, for a query that joins.
///
/// Built from [`COLUMNS`] rather than written out again: two lists in the
/// same order is one list plus a chance to get it wrong.
pub(crate) fn qualified_columns(alias: &str) -> String {
    COLUMNS
        .split(',')
        .map(|column| format!("{alias}.{}", column.trim()))
        .collect::<Vec<_>>()
        .join(", ")
}

/// **Every column here is one the disk owns.** The organisation — rating,
/// label, flag, title, description — is deliberately absent from the update:
/// a rescan reads the file again and must not overwrite what a person said
/// about it. Touching a photograph on disk would otherwise clear its stars.
const UPSERT: &str = "
    INSERT INTO photos(path, folder, file_size, modified_at, taken_at, width, height,
                       orientation, camera, lens, latitude, longitude, place_verdict,
                       place_reason, indexed)
    VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, 1)
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
        latitude = excluded.latitude,
        longitude = excluded.longitude,
        place_verdict = excluded.place_verdict,
        place_reason = excluded.place_reason,
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
    /// The connection, for the parts of the catalogue that live in another
    /// module. [`crate::people`] is the face half of this same repository —
    /// v1 split it the same way and for the same reason, that one file
    /// holding everything is a file nobody reads.
    pub(crate) fn connection(&self) -> &Connection {
        &self.conn
    }

    pub(crate) fn connection_mut(&mut self) -> &mut Connection {
        &mut self.conn
    }

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
        self.conn
            .execute(UPSERT, rusqlite::params_from_iter(upsert_params(photo)))?;
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
                statement.execute(rusqlite::params_from_iter(upsert_params(photo)))?;
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
                photo.people = self.people_of(photo.id)?;
                photo.expressions = self.expressions_of(photo.id)?;
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
        let folder_path = folder;
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

        // Who is on each of them, and how their faces scored: one query for
        // the folder apiece, the same rule the keywords follow. One query a
        // tile would be seven thousand round trips to draw a badge.
        let mut tagged = self.people_by_photo(folder_path, recursive)?;
        let mut expressions = self.expressions_by_photo(folder_path, recursive)?;
        for photo in &mut photos {
            if let Some(people) = tagged.remove(&photo.id) {
                photo.people = people;
            }

            if let Some(summary) = expressions.remove(&photo.id) {
                photo.expressions = summary;
            }
        }

        Ok(photos)
    }

    /// Takes what the file says, but only where the catalogue says nothing.
    ///
    /// A library that has been used before arrives with ratings and titles
    /// already in the files — v1 put them there, and so did whatever anybody
    /// used before that. Showing a photograph as unrated when the file says
    /// five stars is wrong, and so is overwriting what somebody has since
    /// said here. So the file seeds an empty field and never touches a full
    /// one; the condition is in the statement rather than in a read followed
    /// by a write, so two of these cannot race.
    ///
    /// Returns whether anything was taken.
    pub fn seed(&mut self, photo: PhotoId, from: &Organisation) -> Result<bool> {
        let transaction = self.conn.transaction()?;
        let mut taken = transaction.execute(
            "UPDATE photos SET
                 rating = CASE WHEN rating = 0 THEN ?2 ELSE rating END,
                 label  = CASE WHEN label  = 0 THEN ?3 ELSE label  END,
                 title  = CASE WHEN title  IS NULL THEN ?4 ELSE title END,
                 description = CASE WHEN description IS NULL THEN ?5 ELSE description END
             WHERE id = ?1
               AND (   (rating = 0 AND ?2 <> 0)
                    OR (label  = 0 AND ?3 <> 0)
                    OR (title  IS NULL AND ?4 IS NOT NULL)
                    OR (description IS NULL AND ?5 IS NOT NULL))",
            params![
                photo.0,
                from.rating.min(Organisation::MAX_RATING) as i64,
                from.label.as_i64(),
                blank_to_none(from.title.as_deref()),
                blank_to_none(from.description.as_deref()),
            ],
        )?;

        // Keywords are all or nothing: a photograph that already carries some
        // has been spoken about, and merging a file's list into it would put
        // back whatever somebody has just taken off.
        if !from.keywords.is_empty() {
            let has: i64 = transaction.query_row(
                "SELECT COUNT(*) FROM keywords WHERE photo_id = ?1",
                params![photo.0],
                |row| row.get(0),
            )?;
            if has == 0 {
                let mut statement = transaction
                    .prepare("INSERT OR IGNORE INTO keywords(photo_id, keyword) VALUES(?1, ?2)")?;
                for keyword in tidy_keywords(&from.keywords) {
                    statement.execute(params![photo.0, keyword])?;
                    taken += 1;
                }
            }
        }

        transaction.commit()?;
        Ok(taken > 0)
    }

    /// Queues photographs to be written into.
    ///
    /// Called with the same selection the change was applied to, in the same
    /// breath, so the queue and the catalogue move together.
    pub fn enqueue(&mut self, photos: &[PhotoId], now: i64) -> Result<()> {
        if photos.is_empty() {
            return Ok(());
        }

        let transaction = self.conn.transaction()?;
        {
            // A fresh change clears the attempts: whatever stopped the last
            // write — a file open elsewhere, a drive not there — may well be
            // over, and making somebody wait out a backoff they cannot see
            // is not reasonable.
            let mut statement = transaction.prepare(
                "INSERT INTO metadata_outbox(photo_id, not_before, attempts, last_error)
                 VALUES(?1, ?2, 0, NULL)
                 ON CONFLICT(photo_id) DO UPDATE SET
                     not_before = ?2, attempts = 0, last_error = NULL",
            )?;
            for photo in photos {
                statement.execute(params![photo.0, now])?;
            }
        }

        transaction.commit()?;
        Ok(())
    }

    /// What is due to be written, and what to write.
    ///
    /// The organisation comes out of the catalogue here and not out of the
    /// queue, which is what makes a retry write the truth as it stands.
    pub fn due(&self, now: i64, limit: usize) -> Result<Vec<Pending>> {
        let mut statement = self.conn.prepare(&format!(
            "SELECT {COLUMNS}, o.attempts FROM metadata_outbox o
             JOIN photos p ON p.id = o.photo_id
             WHERE o.not_before <= ?1 AND o.attempts < ?2
             ORDER BY o.not_before
             LIMIT ?3"
        ))?;
        let mut pending = statement
            .query_map(params![now, MAX_ATTEMPTS, limit as i64], |row| {
                Ok(Pending {
                    photo: read_photo(row)?,
                    regions: None,
                    attempts: row.get(COLUMN_COUNT)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;

        for entry in &mut pending {
            entry.photo.organisation.keywords = self.keywords_of(entry.photo.id)?;
            entry.regions = self.regions_of(entry.photo.id)?;
        }

        Ok(pending)
    }

    /// The file now says what the catalogue says.
    ///
    /// The row's length and write time are brought up to date in the same
    /// breath. Writing metadata changes both, and leaving them stale would
    /// have the next scan read the whole file again to learn what we just
    /// put there ourselves.
    pub fn written(&mut self, photo: PhotoId, identity: &FileIdentity) -> Result<()> {
        let transaction = self.conn.transaction()?;
        transaction.execute(
            "DELETE FROM metadata_outbox WHERE photo_id = ?1",
            params![photo.0],
        )?;
        transaction.execute(
            "UPDATE photos SET file_size = ?2, modified_at = ?3 WHERE id = ?1",
            params![photo.0, identity.file_size as i64, identity.modified_at],
        )?;
        transaction.commit()?;
        Ok(())
    }

    /// It could not be written. Says why, and when to try again.
    pub fn write_failed(&self, photo: PhotoId, now: i64, error: &str) -> Result<()> {
        self.conn.execute(
            "UPDATE metadata_outbox
             SET attempts = attempts + 1,
                 last_error = ?3,
                 not_before = ?2 + (attempts + 1) * 120
             WHERE photo_id = ?1",
            params![photo.0, now, error],
        )?;
        Ok(())
    }

    /// How many photographs are waiting, and how many have given up.
    pub fn outbox(&self) -> Result<(i64, i64)> {
        Ok(self.conn.query_row(
            "SELECT COUNT(*), COALESCE(SUM(attempts >= ?1), 0) FROM metadata_outbox",
            params![MAX_ATTEMPTS],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?)
    }

    /// What went wrong, for whoever has to be told.
    pub fn outbox_failures(&self, limit: usize) -> Result<Vec<(PathBuf, String)>> {
        let mut statement = self.conn.prepare(
            "SELECT p.path, o.last_error FROM metadata_outbox o
             JOIN photos p ON p.id = o.photo_id
             WHERE o.attempts >= ?1 AND o.last_error IS NOT NULL
             ORDER BY p.path LIMIT ?2",
        )?;
        Ok(statement
            .query_map(params![MAX_ATTEMPTS, limit as i64], |row| {
                Ok((PathBuf::from(row.get::<_, String>(0)?), row.get(1)?))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?)
    }

    /// A photograph is now somewhere else.
    ///
    /// **The row moves with the file.** The rating, the label, the words and
    /// the keywords all hang off its number, so forgetting the old path and
    /// writing a new row would quietly lose an afternoon of culling to a
    /// rename — which is a thing people do constantly and would never think
    /// to be careful about.
    ///
    /// Anything already sitting at the destination is forgotten first: the
    /// file there has been replaced, and so has whatever was said about it.
    pub fn moved(&mut self, from: &Path, to: &Path) -> Result<()> {
        let folder = to
            .parent()
            .map(|parent| parent.to_string_lossy().into_owned())
            .unwrap_or_default();

        let transaction = self.conn.transaction()?;
        transaction.execute(
            "DELETE FROM photos WHERE path = ?1 AND path <> ?2",
            params![to.to_string_lossy(), from.to_string_lossy()],
        )?;
        transaction.execute(
            "UPDATE photos SET path = ?2, folder = ?3 WHERE path = ?1",
            params![from.to_string_lossy(), to.to_string_lossy(), folder],
        )?;
        transaction.commit()?;
        Ok(())
    }

    /// A whole folder is now somewhere else, and everything under it with it.
    ///
    /// Returns how many rows followed. Done in the catalogue rather than by
    /// rescanning both places, so that moving a folder of five thousand
    /// photographs does not mean reading five thousand headers again — and
    /// so that nothing said about them is lost on the way.
    pub fn moved_folder(&mut self, from: &Path, to: &Path) -> Result<usize> {
        let from_text = from.to_string_lossy().into_owned();
        let to_text = to.to_string_lossy().into_owned();
        let under = format!("{}{}", from_text.trim_end_matches(SEPARATOR), SEPARATOR);
        let cut = under.chars().count() as i64;

        let transaction = self.conn.transaction()?;
        let moved = transaction.execute(
            "UPDATE photos SET
                 path   = ?2 || ?4 || substr(path, ?5),
                 folder = ?2 || CASE
                     WHEN length(folder) <= ?6 THEN ''
                     ELSE ?4 || substr(folder, ?5)
                 END
             WHERE folder = ?1 OR substr(folder, 1, ?6) = ?3",
            params![
                from_text,
                to_text,
                under,
                SEPARATOR.to_string(),
                cut + 1,
                cut,
            ],
        )?;
        transaction.commit()?;
        Ok(moved)
    }

    /// These photographs are gone.
    ///
    /// The keywords and any pending write go with them, which the foreign
    /// keys do on their own.
    pub fn forget(&mut self, paths: &[PathBuf]) -> Result<()> {
        if paths.is_empty() {
            return Ok(());
        }

        let transaction = self.conn.transaction()?;
        {
            let mut statement = transaction.prepare("DELETE FROM photos WHERE path = ?1")?;
            for path in paths {
                statement.execute(params![path.to_string_lossy()])?;
            }
        }

        transaction.commit()?;
        Ok(())
    }

    /// The number a path lives under, and nothing else.
    ///
    /// Cheaper than [`Catalog::by_path`] where only the identity is wanted:
    /// that one fetches the row and then its keywords, which is two queries
    /// for an answer of one number.
    pub fn id_of(&self, path: &Path) -> Result<Option<PhotoId>> {
        Ok(self
            .conn
            .query_row(
                "SELECT id FROM photos WHERE path = ?1",
                params![path.to_string_lossy()],
                |row| row.get(0),
            )
            .optional()?
            .map(PhotoId))
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
    /// A title, for one photograph or for a whole selection.
    ///
    /// A slice and not a single row, the same as the rating and the label:
    /// somebody who chose forty photographs and typed a title meant all
    /// forty, and there is no sense in one of these three taking a list and
    /// the others not.
    pub fn set_title(&mut self, photos: &[PhotoId], title: Option<&str>) -> Result<()> {
        self.write_each(
            photos,
            "UPDATE photos SET title = ?2 WHERE id = ?1",
            blank_to_none(title),
        )
    }

    pub fn set_description(&mut self, photos: &[PhotoId], description: Option<&str>) -> Result<()> {
        self.write_each(
            photos,
            "UPDATE photos SET description = ?2 WHERE id = ?1",
            blank_to_none(description),
        )
    }

    /// Where a photograph was taken, said by a person rather than read off
    /// the file.
    ///
    /// The verdict goes with it, and it is always [`Verdict::Precise`] with
    /// no reason: somebody looked at a map and typed it, and that is the
    /// most trustworthy source there is. The mark clears itself, which is
    /// the point of being able to correct one at all.
    ///
    /// A rescan will read the file again and overwrite this — which is
    /// right, because the correction is written **into** the file and read
    /// back from it, exactly the way a rating is.
    pub fn set_place(&mut self, photos: &[PhotoId], place: Option<Place>) -> Result<()> {
        if photos.is_empty() {
            return Ok(());
        }

        let transaction = self.conn.transaction()?;
        {
            let mut statement = transaction.prepare(
                "UPDATE photos
                    SET latitude = ?2, longitude = ?3,
                        place_verdict = ?4, place_reason = 0
                  WHERE id = ?1",
            )?;
            let verdict = match place {
                Some(_) => Verdict::Precise,
                None => Verdict::Nowhere,
            };
            for photo in photos {
                statement.execute(params![
                    photo.0,
                    place.map(|place| place.latitude),
                    place.map(|place| place.longitude),
                    verdict.as_number(),
                ])?;
            }
        }

        transaction.commit()?;
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

/// A photograph waiting to be written into, and what to write.
#[derive(Debug, Clone, PartialEq)]
pub struct Pending {
    pub photo: Photo,
    /// The named face rectangles, rebuilt from the catalogue like everything
    /// else here. `None` is "nobody has face-scanned this", and then another
    /// program's frames in it are left alone; an empty list is "we looked
    /// and nobody here is named", which is how a face taken off somebody
    /// comes back out of the file.
    pub regions: Option<Vec<crate::people::Region>>,
    pub attempts: i64,
}

/// How many columns [`COLUMNS`] names, so a query can add its own after them.
const COLUMN_COUNT: usize = 20;

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
    /// Where the file says it was taken, and how much of that to believe.
    /// Both come off the disk, so both are overwritten by a rescan — which
    /// is right: a correction is written into the file, and read back from
    /// it, exactly the way a rating is.
    pub place: Option<Place>,
    pub verdict: Verdict,
    pub reason: Option<Reason>,
}

/// Everything [`UPSERT`] wants, in its order.
///
/// **One list, used by both the single write and the batch.** They each had
/// their own and it went exactly as one would expect: a column added to one
/// and not the other, and a batch that failed with "got 10, needed 14" only
/// once a test happened to use it.
fn upsert_params(photo: &NewPhoto) -> [rusqlite::types::Value; UPSERT_PARAMS] {
    use rusqlite::types::Value;

    let folder = photo
        .path
        .parent()
        .map(|parent| parent.to_string_lossy().into_owned())
        .unwrap_or_default();
    let number = |value: Option<u32>| match value {
        Some(value) => Value::Integer(i64::from(value)),
        None => Value::Null,
    };
    let degrees = |value: Option<f64>| match value {
        Some(value) => Value::Real(value),
        None => Value::Null,
    };
    let text = |value: Option<&String>| match value {
        Some(value) => Value::Text(value.clone()),
        None => Value::Null,
    };

    [
        Value::Text(photo.path.to_string_lossy().into_owned()),
        Value::Text(folder),
        Value::Integer(photo.file_size as i64),
        Value::Integer(photo.modified_at),
        match photo.taken_at {
            Some(taken) => Value::Integer(taken),
            None => Value::Null,
        },
        number(photo.width),
        number(photo.height),
        Value::Integer(i64::from(photo.orientation)),
        text(photo.camera.as_ref()),
        text(photo.lens.as_ref()),
        degrees(photo.place.map(|place| place.latitude)),
        degrees(photo.place.map(|place| place.longitude)),
        Value::Integer(photo.verdict.as_number()),
        Value::Integer(photo.reason.map(Reason::as_number).unwrap_or(0)),
    ]
}

/// How many values [`UPSERT`] binds. Guarded by a test against the statement
/// itself, so the two cannot drift.
const UPSERT_PARAMS: usize = 14;

/// Builds a photograph out of one row of [`COLUMNS`].
///
/// Keywords are not here: they live in their own table, and asking for them
/// per row would be one query per tile. Whoever wants them fills them in
/// afterwards, for the whole folder at once.
pub(crate) fn read_photo(row: &rusqlite::Row<'_>) -> rusqlite::Result<Photo> {
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
        place: row
            .get::<_, Option<f64>>(16)?
            .zip(row.get::<_, Option<f64>>(17)?)
            .and_then(|(latitude, longitude)| Place::new(latitude, longitude)),
        verdict: Verdict::from_number(row.get(18)?),
        reason: Reason::from_number(row.get(19)?),
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
        // Filled in for a whole folder at once, by whoever wants them.
        people: Vec::new(),
        expressions: Default::default(),
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
            place: None,
            verdict: crate::place::Verdict::Nowhere,
            reason: None,
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

    /// The count is used to reach past the photograph's own columns in a
    /// query that adds its own. Getting it wrong reads the wrong column, and
    /// nothing else would notice.
    #[test]
    fn the_column_count_matches_the_column_list() {
        assert_eq!(COLUMNS.split(',').count(), COLUMN_COUNT);
    }

    /// The write side of the same worry, and the one that actually bit: the
    /// single upsert and the batch each had their own parameter list, and a
    /// column went into one of them.
    #[test]
    fn the_upsert_binds_as_many_values_as_it_asks_for() {
        let highest = (1..=64)
            .filter(|number| UPSERT.contains(&format!("?{number}")))
            .max()
            .expect("the statement binds nothing at all");
        assert_eq!(highest, UPSERT_PARAMS, "{UPSERT}");
        assert_eq!(
            upsert_params(&NewPhoto {
                path: PathBuf::from("/a/b.jpg"),
                file_size: 1,
                modified_at: 2,
                taken_at: None,
                width: None,
                height: None,
                orientation: 1,
                camera: None,
                lens: None,
                place: None,
                verdict: crate::place::Verdict::Nowhere,
                reason: None,
            })
            .len(),
            UPSERT_PARAMS
        );
    }

    #[test]
    fn queueing_the_same_photograph_twice_leaves_one_entry() {
        let mut catalog = Catalog::in_memory().unwrap();
        let id = catalog.upsert(&sample("/a/b.jpg")).unwrap();
        catalog.enqueue(&[id], 100).unwrap();
        catalog.enqueue(&[id], 200).unwrap();
        assert_eq!(catalog.outbox().unwrap(), (1, 0));
    }

    /// The reason there is no payload. A retry writes what the catalogue says
    /// at the moment of writing, not what it said when the change was made.
    #[test]
    fn what_is_due_carries_what_the_catalogue_says_now() {
        let mut catalog = Catalog::in_memory().unwrap();
        let id = catalog.upsert(&sample("/a/b.jpg")).unwrap();
        catalog.set_rating(&[id], 2).unwrap();
        catalog.enqueue(&[id], 100).unwrap();

        // Somebody changes their mind before the write ever happens.
        catalog.set_rating(&[id], 5).unwrap();
        catalog.add_keywords(&[id], &["holiday"]).unwrap();

        let due = catalog.due(200, 10).unwrap();
        assert_eq!(due.len(), 1);
        assert_eq!(due[0].photo.organisation.rating, 5);
        assert_eq!(due[0].photo.organisation.keywords, ["holiday"]);
        assert_eq!(due[0].attempts, 0);
    }

    #[test]
    fn nothing_is_due_before_its_time() {
        let mut catalog = Catalog::in_memory().unwrap();
        let id = catalog.upsert(&sample("/a/b.jpg")).unwrap();
        catalog.enqueue(&[id], 500).unwrap();
        assert!(catalog.due(499, 10).unwrap().is_empty());
        assert_eq!(catalog.due(500, 10).unwrap().len(), 1);
    }

    #[test]
    fn a_written_photograph_leaves_the_queue_and_its_row_is_brought_up_to_date() {
        let mut catalog = Catalog::in_memory().unwrap();
        let id = catalog.upsert(&sample("/a/b.jpg")).unwrap();
        catalog.enqueue(&[id], 100).unwrap();

        // Writing metadata changes the file's length and write time; leaving
        // them stale would have the next scan read the whole file again to
        // learn what we put there ourselves.
        let after = FileIdentity {
            path: PathBuf::from("/a/b.jpg"),
            file_size: 9999,
            modified_at: 1_800_000_000,
        };
        catalog.written(id, &after).unwrap();

        assert_eq!(catalog.outbox().unwrap(), (0, 0));
        let photo = catalog.by_path(Path::new("/a/b.jpg")).unwrap().unwrap();
        assert_eq!(photo.file_size, 9999);
        assert!(catalog.is_current(&after).unwrap());
    }

    #[test]
    fn a_failure_says_why_and_waits_before_trying_again() {
        let mut catalog = Catalog::in_memory().unwrap();
        let id = catalog.upsert(&sample("/a/b.jpg")).unwrap();
        catalog.enqueue(&[id], 100).unwrap();
        catalog
            .write_failed(id, 100, "the file is open elsewhere")
            .unwrap();

        assert!(catalog.due(150, 10).unwrap().is_empty(), "no backoff");
        let due = catalog.due(1000, 10).unwrap();
        assert_eq!(due.len(), 1);
        assert_eq!(due[0].attempts, 1);
    }

    #[test]
    fn a_photograph_that_keeps_failing_stops_being_offered_but_is_not_forgotten() {
        let mut catalog = Catalog::in_memory().unwrap();
        let id = catalog.upsert(&sample("/a/b.jpg")).unwrap();
        catalog.enqueue(&[id], 0).unwrap();
        for _ in 0..MAX_ATTEMPTS {
            catalog.write_failed(id, 0, "no").unwrap();
        }

        assert!(catalog.due(i64::MAX / 2, 10).unwrap().is_empty());
        assert_eq!(catalog.outbox().unwrap(), (1, 1), "it was thrown away");
        let failures = catalog.outbox_failures(10).unwrap();
        assert_eq!(failures.len(), 1);
        assert_eq!(failures[0].1, "no");
    }

    /// A drive that was not there may be there now. Making somebody wait out
    /// a backoff they cannot see would be unreasonable.
    #[test]
    fn a_fresh_change_gives_a_given_up_photograph_another_go() {
        let mut catalog = Catalog::in_memory().unwrap();
        let id = catalog.upsert(&sample("/a/b.jpg")).unwrap();
        catalog.enqueue(&[id], 0).unwrap();
        for _ in 0..MAX_ATTEMPTS {
            catalog.write_failed(id, 0, "no").unwrap();
        }

        catalog.enqueue(&[id], 1000).unwrap();
        assert_eq!(catalog.due(1000, 10).unwrap().len(), 1);
        assert_eq!(catalog.outbox().unwrap(), (1, 0));
    }

    #[test]
    fn a_deleted_photograph_takes_its_queue_entry_with_it() {
        let mut catalog = Catalog::in_memory().unwrap();
        let id = catalog.upsert(&sample("/a/b.jpg")).unwrap();
        catalog.enqueue(&[id], 0).unwrap();
        catalog
            .conn
            .execute("DELETE FROM photos WHERE id = ?1", params![id.0])
            .unwrap();
        assert_eq!(catalog.outbox().unwrap(), (0, 0));
    }

    /// The one this is all for. People rename files constantly and would
    /// never think to be careful about it.
    #[test]
    fn a_renamed_photograph_keeps_everything_said_about_it() {
        let mut catalog = Catalog::in_memory().unwrap();
        let id = catalog.upsert(&sample("/a/old.jpg")).unwrap();
        catalog.set_rating(&[id], 5).unwrap();
        catalog.set_label(&[id], ColorLabel::Purple).unwrap();
        catalog.set_title(&[id], Some("Sunrise")).unwrap();
        catalog.add_keywords(&[id], &["Hawaii"]).unwrap();

        catalog
            .moved(Path::new("/a/old.jpg"), Path::new("/a/new.jpg"))
            .unwrap();

        assert!(catalog.by_path(Path::new("/a/old.jpg")).unwrap().is_none());
        let photo = catalog.by_path(Path::new("/a/new.jpg")).unwrap().unwrap();
        assert_eq!(photo.id, id, "a new row was started");
        assert_eq!(photo.organisation.rating, 5);
        assert_eq!(photo.organisation.label, ColorLabel::Purple);
        assert_eq!(photo.organisation.title.as_deref(), Some("Sunrise"));
        assert_eq!(photo.organisation.keywords, ["Hawaii"]);
    }

    #[test]
    fn moving_a_photograph_to_another_folder_takes_its_folder_with_it() {
        let mut catalog = Catalog::in_memory().unwrap();
        let sep = SEPARATOR;
        catalog
            .upsert(&sample(&format!("{sep}a{sep}one.jpg")))
            .unwrap();
        catalog
            .moved(
                Path::new(&format!("{sep}a{sep}one.jpg")),
                Path::new(&format!("{sep}b{sep}one.jpg")),
            )
            .unwrap();

        assert!(
            catalog
                .in_folder(Path::new(&format!("{sep}a")), false)
                .unwrap()
                .is_empty()
        );
        assert_eq!(
            catalog
                .in_folder(Path::new(&format!("{sep}b")), false)
                .unwrap()
                .len(),
            1
        );
    }

    #[test]
    fn moving_onto_something_replaces_what_was_there() {
        let mut catalog = Catalog::in_memory().unwrap();
        let keep = catalog.upsert(&sample("/a/one.jpg")).unwrap();
        let gone = catalog.upsert(&sample("/a/two.jpg")).unwrap();
        catalog.set_rating(&[gone], 5).unwrap();

        catalog
            .moved(Path::new("/a/one.jpg"), Path::new("/a/two.jpg"))
            .unwrap();

        assert_eq!(catalog.count().unwrap(), 1);
        let photo = catalog.by_path(Path::new("/a/two.jpg")).unwrap().unwrap();
        assert_eq!(photo.id, keep);
        assert_eq!(photo.organisation.rating, 0, "the replaced row won");
    }

    #[test]
    fn a_moved_folder_takes_everything_under_it() {
        let mut catalog = Catalog::in_memory().unwrap();
        let sep = SEPARATOR;
        let id = catalog
            .upsert(&sample(&format!("{sep}a{sep}one.jpg")))
            .unwrap();
        catalog.set_rating(&[id], 4).unwrap();
        catalog
            .upsert(&sample(&format!("{sep}a{sep}deeper{sep}two.jpg")))
            .unwrap();
        // A sibling that merely starts the same way stays where it is.
        catalog
            .upsert(&sample(&format!("{sep}a_side{sep}three.jpg")))
            .unwrap();

        let moved = catalog
            .moved_folder(
                Path::new(&format!("{sep}a")),
                Path::new(&format!("{sep}moved")),
            )
            .unwrap();
        assert_eq!(moved, 2);

        let there = catalog
            .in_folder(Path::new(&format!("{sep}moved")), true)
            .unwrap();
        assert_eq!(there.len(), 2);
        assert_eq!(
            there
                .iter()
                .find(|photo| photo.path.ends_with("one.jpg"))
                .unwrap()
                .organisation
                .rating,
            4
        );
        assert!(
            there
                .iter()
                .any(|photo| photo.path.ends_with(format!("deeper{sep}two.jpg"))),
            "the subfolder did not follow: {:?}",
            there.iter().map(|p| p.path.clone()).collect::<Vec<_>>()
        );
        assert_eq!(
            catalog
                .in_folder(Path::new(&format!("{sep}a_side")), false)
                .unwrap()
                .len(),
            1,
            "a folder that merely starts the same way was moved too"
        );
    }

    #[test]
    fn a_forgotten_photograph_takes_its_keywords_and_its_pending_write() {
        let mut catalog = Catalog::in_memory().unwrap();
        let id = catalog.upsert(&sample("/a/b.jpg")).unwrap();
        catalog.add_keywords(&[id], &["Hawaii"]).unwrap();
        catalog.enqueue(&[id], 0).unwrap();

        catalog.forget(&[PathBuf::from("/a/b.jpg")]).unwrap();
        assert_eq!(catalog.count().unwrap(), 0);
        assert_eq!(catalog.outbox().unwrap(), (0, 0));
        assert!(catalog.keywords_of(id).unwrap().is_empty());
    }

    #[test]
    fn what_the_file_says_fills_in_what_the_catalogue_does_not() {
        let mut catalog = Catalog::in_memory().unwrap();
        let id = catalog.upsert(&sample("/a/b.jpg")).unwrap();

        let from_file = Organisation {
            rating: 5,
            label: ColorLabel::Red,
            flag: Flag::None,
            title: Some("From the file".to_owned()),
            description: None,
            keywords: vec!["Prague".to_owned()],
        };
        assert!(catalog.seed(id, &from_file).unwrap());

        let photo = catalog.by_path(Path::new("/a/b.jpg")).unwrap().unwrap();
        assert_eq!(photo.organisation.rating, 5);
        assert_eq!(photo.organisation.label, ColorLabel::Red);
        assert_eq!(photo.organisation.title.as_deref(), Some("From the file"));
        assert_eq!(photo.organisation.keywords, ["Prague"]);
    }

    /// The other half, and the one that would be expensive to get wrong:
    /// what somebody said here beats what the file says.
    #[test]
    fn what_the_catalogue_already_says_is_not_overwritten_by_the_file() {
        let mut catalog = Catalog::in_memory().unwrap();
        let id = catalog.upsert(&sample("/a/b.jpg")).unwrap();
        catalog.set_rating(&[id], 1).unwrap();
        catalog.set_title(&[id], Some("Mine")).unwrap();
        catalog.add_keywords(&[id], &["mine"]).unwrap();

        let from_file = Organisation {
            rating: 5,
            label: ColorLabel::Red,
            flag: Flag::None,
            title: Some("From the file".to_owned()),
            description: Some("Also from the file".to_owned()),
            keywords: vec!["Prague".to_owned()],
        };
        catalog.seed(id, &from_file).unwrap();

        let photo = catalog.by_path(Path::new("/a/b.jpg")).unwrap().unwrap();
        assert_eq!(photo.organisation.rating, 1, "the rating was overwritten");
        assert_eq!(photo.organisation.title.as_deref(), Some("Mine"));
        assert_eq!(photo.organisation.keywords, ["mine"]);
        // What was empty is still filled in: the label and the description
        // had nothing to lose.
        assert_eq!(photo.organisation.label, ColorLabel::Red);
        assert_eq!(
            photo.organisation.description.as_deref(),
            Some("Also from the file")
        );
    }

    #[test]
    fn seeding_from_a_file_that_says_nothing_changes_nothing() {
        let mut catalog = Catalog::in_memory().unwrap();
        let id = catalog.upsert(&sample("/a/b.jpg")).unwrap();
        assert!(!catalog.seed(id, &Organisation::default()).unwrap());
        assert!(
            catalog
                .by_path(Path::new("/a/b.jpg"))
                .unwrap()
                .unwrap()
                .organisation
                .is_empty()
        );
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
        catalog.set_title(&[id], Some("Sunrise")).unwrap();
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
        let mut catalog = Catalog::in_memory().unwrap();
        let id = catalog.upsert(&sample("/a/b.jpg")).unwrap();
        catalog.set_title(&[id], Some("Sunrise")).unwrap();
        catalog.set_title(&[id], Some("   ")).unwrap();
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
