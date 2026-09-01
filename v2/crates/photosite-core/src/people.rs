//! Who is in the photograph.
//!
//! The vocabulary is [CONTEXT.md](../../../CONTEXT.md)'s: a **face** is a
//! rectangle with a vector attached, a **person** is a name, and a
//! **suggestion** is a face that is probably somebody but not certainly.
//!
//! Two things here differ from v1 on purpose.
//!
//! **A face hangs off the photograph's number, not off its path.** v1 keyed
//! its face rows by path, so renaming a file orphaned every face on it and
//! the next scan found them all again as strangers. Here the row travels
//! with the photograph exactly as its stars do, and a deleted photograph
//! takes its faces with it without anybody writing the code to do so.
//!
//! **The catalogue owns the encoding of an embedding.** The face engine
//! produces a vector; how a vector is stored is a question for whatever
//! stores it. So the little-endian blob is here, next to the table it goes
//! into, and the engine knows nothing about databases.

use crate::catalog::Catalog;
use crate::domain::{FileIdentity, Photo, PhotoId};
use anyhow::Result;
use rusqlite::{OptionalExtension as _, params};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Above this a stored probability counts as a smile, and as an open eye.
///
/// The thresholds are here rather than with the models because they are what
/// turns a number into a verdict, and the scan, the aggregate query, the
/// badge and the filter all have to agree. One number in one place is what
/// makes that true.
pub const SMILE_THRESHOLD: f64 = 0.5;
pub const EYES_OPEN_THRESHOLD: f64 = 0.5;

/// A named person, and how many faces the catalogue holds for them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Person {
    pub id: i64,
    pub name: String,
    pub faces: usize,
}

/// A person on one photograph — what a badge, an info row and a filter chip
/// are made of.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tag {
    pub id: i64,
    pub name: String,
}

/// One face in the catalogue.
///
/// The rectangle is a fraction of the frame, so it means the same thing on a
/// tile, in the preview and in the file.
#[derive(Debug, Clone, PartialEq)]
pub struct Face {
    pub id: i64,
    pub photo: PhotoId,
    pub path: PathBuf,
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
    pub confidence: f64,
    pub embedding: Vec<f32>,
    pub person: Option<i64>,
    /// Probably this person. Nothing has been written anywhere on the
    /// strength of it.
    pub suggested: Option<i64>,
    pub smile: Option<f64>,
    pub eyes_open: Option<f64>,
}

impl Face {
    pub fn rectangle(&self) -> (f64, f64, f64, f64) {
        (self.x, self.y, self.width, self.height)
    }

    pub fn is_smiling(&self) -> Option<bool> {
        self.smile.map(|value| value >= SMILE_THRESHOLD)
    }

    pub fn has_eyes_open(&self) -> Option<bool> {
        self.eyes_open.map(|value| value >= EYES_OPEN_THRESHOLD)
    }
}

/// A face as it comes out of a scan, before the catalogue gives it a number.
#[derive(Debug, Clone, PartialEq)]
pub struct Observation {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
    pub confidence: f64,
    pub embedding: Vec<f32>,
    pub person: Option<i64>,
    pub suggested: Option<i64>,
    pub smile: Option<f64>,
    pub eyes_open: Option<f64>,
}

/// How one photograph's faces scored, taken together.
///
/// Two counts and not one ratio, because "nobody has been scored yet" and
/// "everybody was scored and everybody is smiling" are different states and
/// a ratio cannot tell them apart.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Expressions {
    pub faces: usize,
    /// How many of them the expression models have actually seen.
    pub scored: usize,
    pub smiling: usize,
    pub eyes_open: usize,
}

impl Expressions {
    /// Everybody on the photograph is scored and smiling.
    pub fn all_smiling(&self) -> bool {
        self.faces > 0 && self.smiling == self.faces
    }

    pub fn all_eyes_open(&self) -> bool {
        self.faces > 0 && self.eyes_open == self.faces
    }

    /// Somebody who was looked at is not smiling. Measured against `scored`
    /// and never against `faces`: a face nobody has scored is not evidence
    /// of a frown.
    pub fn anyone_not_smiling(&self) -> bool {
        self.smiling < self.scored
    }

    pub fn anyone_blinking(&self) -> bool {
        self.eyes_open < self.scored
    }

    /// Is there anything here worth a badge on a tile?
    pub fn worth_showing(&self) -> bool {
        self.anyone_not_smiling() || self.anyone_blinking()
    }
}

/// One named face rectangle, as it goes into the photograph.
///
/// The name and the rectangle, and nothing about who wrote it: this is what
/// an MWG region is, and the format is what Lightroom, digiKam and Windows
/// read face frames from.
#[derive(Debug, Clone, PartialEq)]
pub struct Region {
    pub name: String,
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

/// The square of a photograph a face's thumbnail is cut out of.
///
/// Returned as fractions of the frame, like everything else about a face,
/// and **square in pixels rather than in fractions** — the frame is not
/// square, so the two are different, and a chip built from the wrong one
/// shows every face stretched.
///
/// The margin is what keeps hair and a chin in the picture: the detector's
/// box stops at the face itself, and a crop tight to it is a portrait of a
/// nose.
pub fn crop(face: (f64, f64, f64, f64), margin: f64, frame: (u32, u32)) -> (f64, f64, f64, f64) {
    let (width, height) = (f64::from(frame.0.max(1)), f64::from(frame.1.max(1)));
    let (x, y, w, h) = face;
    let side = (w * width).max(h * height) * (1.0 + 2.0 * margin.max(0.0));
    let (half_w, half_h) = (side / width / 2.0, side / height / 2.0);
    let (centre_x, centre_y) = (x + w / 2.0, y + h / 2.0);

    // Clamped into the frame rather than allowed to hang over the edge: a
    // texture sampled outside itself shows whatever the edge pixel is,
    // smeared, and a face at the border of a photograph is exactly where
    // that happens.
    let left = (centre_x - half_w).clamp(0.0, (1.0 - 2.0 * half_w).max(0.0));
    let top = (centre_y - half_h).clamp(0.0, (1.0 - 2.0 * half_h).max(0.0));
    (left, top, (2.0 * half_w).min(1.0), (2.0 * half_h).min(1.0))
}

/// A vector as the catalogue stores it.
///
/// Little-endian on every platform, so a catalogue carried from one machine
/// to another reads the same. The native byte order would work everywhere it
/// was written and nowhere else, which is the kind of bug that only ever
/// happens to somebody else.
pub fn to_blob(embedding: &[f32]) -> Vec<u8> {
    embedding
        .iter()
        .flat_map(|value| value.to_le_bytes())
        .collect()
}

pub fn from_blob(blob: &[u8]) -> Vec<f32> {
    blob.as_chunks::<4>()
        .0
        .iter()
        .map(|bytes| f32::from_le_bytes(*bytes))
        .collect()
}

/// What one row of [`FACE_COLUMNS`] holds.
const FACE_COLUMNS: &str = "f.id, f.photo_id, p.path, f.x, f.y, f.w, f.h, f.confidence, \
                            f.embedding, f.person_id, f.suggested_person_id, f.smile, f.eyes_open";

/// A `LIMIT` SQLite will take.
///
/// `usize::MAX as i64` is `-1`, which SQLite happens to read as "no limit" —
/// and a query that works because of a wrap is one that stops working on the
/// day somebody changes the cast.
fn at_most(limit: usize) -> i64 {
    i64::try_from(limit).unwrap_or(i64::MAX)
}

fn read_face(row: &rusqlite::Row<'_>) -> rusqlite::Result<Face> {
    Ok(Face {
        id: row.get(0)?,
        photo: PhotoId(row.get(1)?),
        path: PathBuf::from(row.get::<_, String>(2)?),
        x: row.get(3)?,
        y: row.get(4)?,
        width: row.get(5)?,
        height: row.get(6)?,
        confidence: row.get(7)?,
        embedding: from_blob(&row.get::<_, Vec<u8>>(8)?),
        person: row.get(9)?,
        suggested: row.get(10)?,
        smile: row.get(11)?,
        eyes_open: row.get(12)?,
    })
}

/// The folder predicate, and the arguments that go with it.
///
/// The same `substr` prefix match the rest of the catalogue uses, and for
/// the same reason: `LIKE` would read the underscore in `my_photos` as "any
/// character" and quietly pull in `myXphotos`.
fn under(folder: &Path, recursive: bool) -> (String, String, &'static str) {
    let folder = folder.to_string_lossy().into_owned();
    let under = format!(
        "{}{}",
        folder.trim_end_matches(std::path::MAIN_SEPARATOR),
        std::path::MAIN_SEPARATOR
    );
    let predicate = if recursive {
        "(p.folder = ?1 OR substr(p.folder, 1, length(?2)) = ?2)"
    } else {
        "p.folder = ?1"
    };
    (folder, under, predicate)
}

/// SQLite refuses a parameter the statement never mentions, so the list is
/// built to match the predicate rather than always holding both.
///
/// `&String` and not `&str` on purpose: the list holds `&dyn ToSql`, and it
/// is `String` that implements the trait — a `&str` would need a reference
/// to a reference that outlives the call.
#[allow(clippy::ptr_arg)]
fn arguments<'a>(
    folder: &'a String,
    prefix: &'a String,
    recursive: bool,
) -> Vec<&'a dyn rusqlite::ToSql> {
    if recursive {
        vec![folder, prefix]
    } else {
        vec![folder]
    }
}

impl Catalog {
    /// The photographs in a folder that a face scan has not seen, or has
    /// seen in a state the file is no longer in.
    ///
    /// The comparison is the same one the rest of the application uses for
    /// "has this file changed" — length and write time — so a sweep is
    /// incremental for free and a touched file is looked at again.
    pub fn unscanned_faces(&self, folder: &Path, recursive: bool) -> Result<Vec<Photo>> {
        let (folder, prefix, predicate) = under(folder, recursive);
        let mut statement = self.connection().prepare(&format!(
            "SELECT {columns} FROM photos p
             LEFT JOIN face_scans s ON s.photo_id = p.id
             WHERE {predicate}
               AND (s.photo_id IS NULL
                    OR s.file_size <> p.file_size
                    OR s.modified_at <> p.modified_at)
             ORDER BY p.path",
            columns = crate::catalog::qualified_columns("p")
        ))?;
        let args = arguments(&folder, &prefix, recursive);
        Ok(statement
            .query_map(args.as_slice(), crate::catalog::read_photo)?
            .collect::<rusqlite::Result<Vec<_>>>()?)
    }

    /// Replaces everything known about one photograph's faces, and stamps
    /// the scan — in one transaction, so a crash cannot leave a photograph
    /// marked as scanned with no faces on it.
    pub fn replace_faces(
        &mut self,
        photo: PhotoId,
        identity: &FileIdentity,
        faces: &[Observation],
        now: i64,
    ) -> Result<()> {
        let transaction = self.connection_mut().transaction()?;
        transaction.execute("DELETE FROM faces WHERE photo_id = ?1", params![photo.0])?;
        {
            let mut statement = transaction.prepare(
                "INSERT INTO faces(photo_id, x, y, w, h, confidence, embedding,
                                   person_id, suggested_person_id, smile, eyes_open)
                 VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            )?;
            for face in faces {
                statement.execute(params![
                    photo.0,
                    face.x,
                    face.y,
                    face.width,
                    face.height,
                    face.confidence,
                    to_blob(&face.embedding),
                    face.person,
                    face.suggested,
                    face.smile,
                    face.eyes_open,
                ])?;
            }
        }

        transaction.execute(
            "INSERT INTO face_scans(photo_id, file_size, modified_at, faces, scanned_at)
             VALUES(?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(photo_id) DO UPDATE SET
                 file_size = excluded.file_size,
                 modified_at = excluded.modified_at,
                 faces = excluded.faces,
                 scanned_at = excluded.scanned_at",
            params![
                photo.0,
                identity.file_size as i64,
                identity.modified_at,
                faces.len() as i64,
                now
            ],
        )?;
        transaction.commit()?;
        Ok(())
    }

    /// Every face on one photograph, most confident first.
    pub fn faces_of(&self, photo: PhotoId) -> Result<Vec<Face>> {
        let mut statement = self.connection().prepare(&format!(
            "SELECT {FACE_COLUMNS} FROM faces f JOIN photos p ON p.id = f.photo_id
             WHERE f.photo_id = ?1 ORDER BY f.confidence DESC"
        ))?;
        Ok(statement
            .query_map(params![photo.0], read_face)?
            .collect::<rusqlite::Result<Vec<_>>>()?)
    }

    /// Faces with nobody's name on them, no suggestion pending, and not
    /// waved away as strangers — the pool the unnamed groups are built from.
    pub fn unnamed_faces(&self, limit: usize) -> Result<Vec<Face>> {
        self.query_faces(
            "f.person_id IS NULL AND f.suggested_person_id IS NULL AND f.ignored = 0",
            limit,
        )
    }

    /// Faces waiting for a yes or a no.
    pub fn suggested_faces(&self, limit: usize) -> Result<Vec<Face>> {
        self.query_faces(
            "f.person_id IS NULL AND f.suggested_person_id IS NOT NULL",
            limit,
        )
    }

    pub fn faces_of_person(&self, person: i64, limit: usize) -> Result<Vec<Face>> {
        let mut statement = self.connection().prepare(&format!(
            "SELECT {FACE_COLUMNS} FROM faces f JOIN photos p ON p.id = f.photo_id
             WHERE f.person_id = ?1 ORDER BY f.confidence DESC LIMIT ?2"
        ))?;
        Ok(statement
            .query_map(params![person, at_most(limit)], read_face)?
            .collect::<rusqlite::Result<Vec<_>>>()?)
    }

    fn query_faces(&self, predicate: &str, limit: usize) -> Result<Vec<Face>> {
        let mut statement = self.connection().prepare(&format!(
            "SELECT {FACE_COLUMNS} FROM faces f JOIN photos p ON p.id = f.photo_id
             WHERE {predicate} ORDER BY f.confidence DESC LIMIT ?1"
        ))?;
        Ok(statement
            .query_map(params![at_most(limit)], read_face)?
            .collect::<rusqlite::Result<Vec<_>>>()?)
    }

    /// One vector per named person: the average of their faces.
    ///
    /// This is what a freshly scanned face is compared against. Returned as
    /// plain numbers rather than as the face engine's own type, because the
    /// catalogue has no business knowing which library did the arithmetic.
    pub fn person_embeddings(&self) -> Result<Vec<(i64, Vec<Vec<f32>>)>> {
        let mut statement = self.connection().prepare(
            "SELECT person_id, embedding FROM faces
             WHERE person_id IS NOT NULL ORDER BY person_id",
        )?;
        let mut by_person: Vec<(i64, Vec<Vec<f32>>)> = Vec::new();
        let rows = statement.query_map([], |row| {
            Ok((row.get::<_, i64>(0)?, row.get::<_, Vec<u8>>(1)?))
        })?;
        for row in rows {
            let (person, blob) = row?;
            match by_person.last_mut() {
                Some((id, embeddings)) if *id == person => embeddings.push(from_blob(&blob)),
                _ => by_person.push((person, vec![from_blob(&blob)])),
            }
        }

        Ok(by_person)
    }

    pub fn people(&self) -> Result<Vec<Person>> {
        let mut statement = self.connection().prepare(
            "SELECT people.id, people.name, COUNT(faces.id)
             FROM people LEFT JOIN faces ON faces.person_id = people.id
             GROUP BY people.id, people.name
             ORDER BY people.name COLLATE NOCASE",
        )?;
        Ok(statement
            .query_map([], |row| {
                Ok(Person {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    faces: row.get::<_, i64>(2)? as usize,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?)
    }

    /// Finds a person by name — without regard to case, so "jana" and "Jana"
    /// stay one person — or makes one.
    pub fn person_named(&mut self, name: &str) -> Result<i64> {
        let name = name.trim();
        anyhow::ensure!(!name.is_empty(), "a person needs a name");
        if let Some(id) = self
            .connection()
            .query_row(
                "SELECT id FROM people WHERE name = ?1 COLLATE NOCASE",
                params![name],
                |row| row.get::<_, i64>(0),
            )
            .optional()?
        {
            return Ok(id);
        }

        self.connection()
            .execute("INSERT INTO people(name) VALUES(?1)", params![name])?;
        Ok(self.connection().last_insert_rowid())
    }

    pub fn rename_person(&mut self, person: i64, name: &str) -> Result<()> {
        let name = name.trim();
        anyhow::ensure!(!name.is_empty(), "a person needs a name");
        self.connection().execute(
            "UPDATE people SET name = ?2 WHERE id = ?1",
            params![person, name],
        )?;
        Ok(())
    }

    /// Forgets a person. Their faces stay and return to the unnamed pool.
    ///
    /// Names already written into files are left where they are, which is
    /// the same rule keywords follow: what has been said to the world is not
    /// unsaid by us on a whim. The next time one of those photographs is
    /// written for any other reason, it goes out with the name gone.
    pub fn delete_person(&mut self, person: i64) -> Result<()> {
        let transaction = self.connection_mut().transaction()?;
        transaction.execute(
            "UPDATE faces SET person_id = NULL WHERE person_id = ?1",
            params![person],
        )?;
        transaction.execute(
            "UPDATE faces SET suggested_person_id = NULL WHERE suggested_person_id = ?1",
            params![person],
        )?;
        transaction.execute(
            "DELETE FROM photo_people WHERE person_id = ?1",
            params![person],
        )?;
        transaction.execute("DELETE FROM people WHERE id = ?1", params![person])?;
        transaction.commit()?;
        Ok(())
    }

    /// Puts a name on faces, or takes one off.
    ///
    /// **Deciding settles a face either way.** Any pending suggestion is
    /// cleared in the same statement, and a face given a person is no
    /// stranger any more — otherwise a face could be both named and waiting
    /// to be asked about, and the People window would offer it twice.
    ///
    /// Returns the photographs the faces sit on, because naming somebody
    /// means their name has to be written into those files.
    pub fn assign_faces(&mut self, faces: &[i64], person: Option<i64>) -> Result<Vec<PhotoId>> {
        if faces.is_empty() {
            return Ok(Vec::new());
        }

        let photos = self.photos_of_faces(faces)?;
        let transaction = self.connection_mut().transaction()?;
        {
            let mut statement = transaction.prepare(
                "UPDATE faces
                 SET person_id = ?2, suggested_person_id = NULL, ignored = 0
                 WHERE id = ?1",
            )?;
            for face in faces {
                statement.execute(params![face, person])?;
            }
        }

        transaction.commit()?;
        Ok(photos)
    }

    /// Names faces, and makes the photographs they are on say so.
    ///
    /// This is the whole of "naming a group", in the core where a test can
    /// watch it: the faces take the name, the name goes into each
    /// photograph's keywords, and those photographs are queued to be
    /// written. The face frames follow on their own — the outbox rebuilds
    /// them from the catalogue at the moment of writing.
    ///
    /// The keyword matters more than it looks. It is what makes a search for
    /// somebody's name find their photographs in every other program too,
    /// and it is the only part of this that survives being read by software
    /// that has never heard of face regions.
    pub fn name_faces(&mut self, faces: &[i64], person: i64, now: i64) -> Result<Vec<PhotoId>> {
        let name: String = self.connection().query_row(
            "SELECT name FROM people WHERE id = ?1",
            params![person],
            |row| row.get(0),
        )?;
        let photos = self.assign_faces(faces, Some(person))?;
        self.add_keywords(&photos, &[name])?;
        self.enqueue(&photos, now)?;
        Ok(photos)
    }

    /// Takes the name off faces, and off the photographs where none of that
    /// person's faces are left.
    ///
    /// **The keyword only comes off a photograph when the person really has
    /// gone from it.** Two faces of one person on a group shot is ordinary,
    /// and removing one wrong match must not take their name off a
    /// photograph they are plainly still in.
    pub fn unname_faces(&mut self, faces: &[i64], now: i64) -> Result<Vec<PhotoId>> {
        // Who was on them, before anything is changed.
        let mut was: Vec<(PhotoId, i64, String)> = Vec::new();
        {
            let mut statement = self.connection().prepare(
                "SELECT f.photo_id, people.id, people.name
                 FROM faces f JOIN people ON people.id = f.person_id
                 WHERE f.id = ?1",
            )?;
            for face in faces {
                for row in statement.query_map(params![face], |row| {
                    Ok((PhotoId(row.get(0)?), row.get(1)?, row.get(2)?))
                })? {
                    let entry = row?;
                    if !was.contains(&entry) {
                        was.push(entry);
                    }
                }
            }
        }

        let photos = self.assign_faces(faces, None)?;
        for (photo, person, name) in was {
            let left: i64 = self.connection().query_row(
                "SELECT COUNT(*) FROM (
                     SELECT 1 FROM faces WHERE photo_id = ?1 AND person_id = ?2
                     UNION ALL
                     SELECT 1 FROM photo_people WHERE photo_id = ?1 AND person_id = ?2
                 )",
                params![photo.0, person],
                |row| row.get(0),
            )?;
            if left == 0 {
                self.remove_keywords(&[photo], &[name])?;
            }
        }

        self.enqueue(&photos, now)?;
        Ok(photos)
    }

    /// Waves faces away as strangers.
    ///
    /// The rows stay, so the photographs still count as scanned and the
    /// sweep does not offer them again on every run. Naming one later brings
    /// it straight back.
    pub fn ignore_faces(&mut self, faces: &[i64]) -> Result<()> {
        self.each_face(
            faces,
            "UPDATE faces SET ignored = 1, suggested_person_id = NULL WHERE id = ?1",
        )
    }

    /// No, that is not them. The face returns to the unnamed pool.
    pub fn clear_suggestions(&mut self, faces: &[i64]) -> Result<()> {
        self.each_face(
            faces,
            "UPDATE faces SET suggested_person_id = NULL WHERE id = ?1",
        )
    }

    fn each_face(&mut self, faces: &[i64], sql: &str) -> Result<()> {
        if faces.is_empty() {
            return Ok(());
        }

        let transaction = self.connection_mut().transaction()?;
        {
            let mut statement = transaction.prepare(sql)?;
            for face in faces {
                statement.execute(params![face])?;
            }
        }

        transaction.commit()?;
        Ok(())
    }

    /// Which photographs a set of faces sits on, each once.
    pub fn photos_of_faces(&self, faces: &[i64]) -> Result<Vec<PhotoId>> {
        let mut statement = self
            .connection()
            .prepare("SELECT DISTINCT photo_id FROM faces WHERE id = ?1")?;
        let mut photos = Vec::new();
        for face in faces {
            for row in statement.query_map(params![face], |row| row.get::<_, i64>(0))? {
                let photo = PhotoId(row?);
                if !photos.contains(&photo) {
                    photos.push(photo);
                }
            }
        }

        Ok(photos)
    }

    /// Says by hand that somebody is on a photograph — they are in it, but
    /// turned away, or behind the camera's own strap, and there is no face
    /// for the detector to frame. Saying it twice says it once.
    ///
    /// The keyword goes in exactly as it would for a face, because a person
    /// on a photograph is a person on a photograph however we came to know
    /// it — and a search for their name has no way to tell the two apart.
    pub fn tag_person(&mut self, photo: PhotoId, person: i64, now: i64) -> Result<()> {
        let name: String = self.connection().query_row(
            "SELECT name FROM people WHERE id = ?1",
            params![person],
            |row| row.get(0),
        )?;
        self.connection().execute(
            "INSERT OR IGNORE INTO photo_people(photo_id, person_id) VALUES(?1, ?2)",
            params![photo.0, person],
        )?;
        self.add_keywords(&[photo], &[name])?;
        self.enqueue(&[photo], now)?;
        Ok(())
    }

    /// Takes a person off one photograph entirely: the hand-written tag goes,
    /// and any of their faces on that photograph return to the unnamed pool.
    pub fn untag_person(&mut self, photo: PhotoId, person: i64, now: i64) -> Result<()> {
        let name: String = self.connection().query_row(
            "SELECT name FROM people WHERE id = ?1",
            params![person],
            |row| row.get(0),
        )?;
        let transaction = self.connection_mut().transaction()?;
        transaction.execute(
            "DELETE FROM photo_people WHERE photo_id = ?1 AND person_id = ?2",
            params![photo.0, person],
        )?;
        transaction.execute(
            "UPDATE faces SET person_id = NULL WHERE photo_id = ?1 AND person_id = ?2",
            params![photo.0, person],
        )?;
        transaction.commit()?;
        self.remove_keywords(&[photo], &[name])?;
        self.enqueue(&[photo], now)?;
        Ok(())
    }

    /// Everybody named, per photograph, for a whole folder in one query.
    ///
    /// A face with a name and a hand-written tag count the same: both mean
    /// "this person is on this photograph", which is the only question a
    /// badge, an info row or a filter chip is asking.
    pub fn people_by_photo(
        &self,
        folder: &Path,
        recursive: bool,
    ) -> Result<HashMap<PhotoId, Vec<Tag>>> {
        let (folder, prefix, predicate) = under(folder, recursive);
        let mut statement = self.connection().prepare(&format!(
            "SELECT DISTINCT p.id, people.id, people.name
             FROM photos p
             JOIN (
                 SELECT photo_id, person_id FROM faces WHERE person_id IS NOT NULL
                 UNION
                 SELECT photo_id, person_id FROM photo_people
             ) AS source ON source.photo_id = p.id
             JOIN people ON people.id = source.person_id
             WHERE {predicate}
             ORDER BY people.name COLLATE NOCASE"
        ))?;
        let args = arguments(&folder, &prefix, recursive);
        let mut found: HashMap<PhotoId, Vec<Tag>> = HashMap::new();
        let rows = statement.query_map(args.as_slice(), |row| {
            Ok((
                PhotoId(row.get(0)?),
                Tag {
                    id: row.get(1)?,
                    name: row.get(2)?,
                },
            ))
        })?;
        for row in rows {
            let (photo, tag) = row?;
            found.entry(photo).or_default().push(tag);
        }

        Ok(found)
    }

    /// Who is on one photograph.
    pub fn people_of(&self, photo: PhotoId) -> Result<Vec<Tag>> {
        let mut statement = self.connection().prepare(
            "SELECT DISTINCT people.id, people.name
             FROM (
                 SELECT photo_id, person_id FROM faces WHERE person_id IS NOT NULL
                 UNION
                 SELECT photo_id, person_id FROM photo_people
             ) AS source
             JOIN people ON people.id = source.person_id
             WHERE source.photo_id = ?1
             ORDER BY people.name COLLATE NOCASE",
        )?;
        Ok(statement
            .query_map(params![photo.0], |row| {
                Ok(Tag {
                    id: row.get(0)?,
                    name: row.get(1)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?)
    }

    /// How one photograph's faces scored.
    pub fn expressions_of(&self, photo: PhotoId) -> Result<Expressions> {
        Ok(self.connection().query_row(
            "SELECT COUNT(*),
                    SUM(CASE WHEN smile IS NOT NULL AND eyes_open IS NOT NULL
                             THEN 1 ELSE 0 END),
                    SUM(CASE WHEN smile >= ?2 THEN 1 ELSE 0 END),
                    SUM(CASE WHEN eyes_open >= ?3 THEN 1 ELSE 0 END)
             FROM faces WHERE photo_id = ?1",
            params![photo.0, SMILE_THRESHOLD, EYES_OPEN_THRESHOLD],
            |row| {
                Ok(Expressions {
                    faces: row.get::<_, i64>(0)? as usize,
                    scored: row.get::<_, Option<i64>>(1)?.unwrap_or(0) as usize,
                    smiling: row.get::<_, Option<i64>>(2)?.unwrap_or(0) as usize,
                    eyes_open: row.get::<_, Option<i64>>(3)?.unwrap_or(0) as usize,
                })
            },
        )?)
    }

    /// Every photograph's expression tally, for a whole folder in one query.
    ///
    /// The thresholds go into the statement rather than being applied after
    /// it, so the aggregate and the badge cannot come to different verdicts
    /// about the same stored number.
    pub fn expressions_by_photo(
        &self,
        folder: &Path,
        recursive: bool,
    ) -> Result<HashMap<PhotoId, Expressions>> {
        let (folder, prefix, predicate) = under(folder, recursive);
        let mut statement = self.connection().prepare(&format!(
            "SELECT p.id,
                    COUNT(*),
                    SUM(CASE WHEN f.smile IS NOT NULL AND f.eyes_open IS NOT NULL
                             THEN 1 ELSE 0 END),
                    SUM(CASE WHEN f.smile >= ?{smile} THEN 1 ELSE 0 END),
                    SUM(CASE WHEN f.eyes_open >= ?{eyes} THEN 1 ELSE 0 END)
             FROM faces f JOIN photos p ON p.id = f.photo_id
             WHERE {predicate}
             GROUP BY p.id",
            smile = if recursive { 3 } else { 2 },
            eyes = if recursive { 4 } else { 3 },
        ))?;
        let mut args = arguments(&folder, &prefix, recursive);
        args.push(&SMILE_THRESHOLD);
        args.push(&EYES_OPEN_THRESHOLD);

        let mut found = HashMap::new();
        let rows = statement.query_map(args.as_slice(), |row| {
            Ok((
                PhotoId(row.get(0)?),
                Expressions {
                    faces: row.get::<_, i64>(1)? as usize,
                    scored: row.get::<_, i64>(2)? as usize,
                    smiling: row.get::<_, i64>(3)? as usize,
                    eyes_open: row.get::<_, i64>(4)? as usize,
                },
            ))
        })?;
        for row in rows {
            let (photo, expressions) = row?;
            found.insert(photo, expressions);
        }

        Ok(found)
    }

    /// The photographs in a folder whose faces predate the expression
    /// models — the work list of the pass that scores them without touching
    /// who they are.
    pub fn missing_expressions(&self, folder: &Path, recursive: bool) -> Result<Vec<Photo>> {
        let (folder, prefix, predicate) = under(folder, recursive);
        let mut statement = self.connection().prepare(&format!(
            "SELECT DISTINCT {columns} FROM photos p
             JOIN faces f ON f.photo_id = p.id
             WHERE {predicate} AND (f.smile IS NULL OR f.eyes_open IS NULL)
             ORDER BY p.path",
            columns = crate::catalog::qualified_columns("p")
        ))?;
        let args = arguments(&folder, &prefix, recursive);
        Ok(statement
            .query_map(args.as_slice(), crate::catalog::read_photo)?
            .collect::<rusqlite::Result<Vec<_>>>()?)
    }

    /// Writes expression scores onto faces that already exist.
    ///
    /// **Their numbers and their names are untouched.** This is the whole
    /// point of a separate pass: a photograph scanned before the expression
    /// models existed must be scored without its people being forgotten and
    /// found again as strangers.
    pub fn score_expressions(&mut self, scores: &[(i64, Option<f64>, Option<f64>)]) -> Result<()> {
        if scores.is_empty() {
            return Ok(());
        }

        let transaction = self.connection_mut().transaction()?;
        {
            let mut statement =
                transaction.prepare("UPDATE faces SET smile = ?2, eyes_open = ?3 WHERE id = ?1")?;
            for (face, smile, eyes_open) in scores {
                statement.execute(params![face, smile, eyes_open])?;
            }
        }

        transaction.commit()?;
        Ok(())
    }

    /// The named face rectangles of one photograph, as they go into the file.
    ///
    /// Built from the catalogue every time rather than kept anywhere,
    /// exactly as the rest of the outbox works: what gets written is what
    /// the catalogue says at the moment of writing, so a rename that has
    /// happened since is already in it.
    ///
    /// **`None` means "nobody has looked here".** A photograph that has
    /// never been face-scanned may well carry frames another program wrote,
    /// and clearing those because somebody pressed a star would be
    /// destroying their work on the way past. An empty list is the other
    /// answer entirely: we looked, and nobody on this photograph is named.
    pub fn regions_of(&self, photo: PhotoId) -> Result<Option<Vec<Region>>> {
        let scanned: bool = self.connection().query_row(
            "SELECT EXISTS(SELECT 1 FROM face_scans WHERE photo_id = ?1)",
            params![photo.0],
            |row| row.get(0),
        )?;
        if !scanned {
            return Ok(None);
        }

        let mut statement = self.connection().prepare(
            "SELECT people.name, f.x, f.y, f.w, f.h
             FROM faces f JOIN people ON people.id = f.person_id
             WHERE f.photo_id = ?1
             ORDER BY f.confidence DESC",
        )?;
        Ok(Some(
            statement
                .query_map(params![photo.0], |row| {
                    Ok(Region {
                        name: row.get(0)?,
                        x: row.get(1)?,
                        y: row.get(2)?,
                        width: row.get(3)?,
                        height: row.get(4)?,
                    })
                })?
                .collect::<rusqlite::Result<Vec<_>>>()?,
        ))
    }

    /// How many faces and how many people the catalogue holds, for the
    /// diagnostics window.
    pub fn face_counts(&self) -> Result<(i64, i64, i64)> {
        Ok(self.connection().query_row(
            "SELECT
                 (SELECT COUNT(*) FROM faces),
                 (SELECT COUNT(*) FROM faces WHERE person_id IS NOT NULL),
                 (SELECT COUNT(*) FROM people)",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::NewPhoto;
    use crate::place::Verdict;

    fn catalog_with(paths: &[&str]) -> Catalog {
        let catalog = Catalog::in_memory().unwrap();
        for path in paths {
            catalog
                .upsert(&NewPhoto {
                    path: PathBuf::from(path),
                    file_size: 1000,
                    modified_at: 10,
                    taken_at: None,
                    width: Some(6000),
                    height: Some(4000),
                    orientation: 1,
                    camera: None,
                    lens: None,
                    place: None,
                    verdict: Verdict::Nowhere,
                    reason: None,
                })
                .unwrap();
        }

        catalog
    }

    fn observation(x: f64, embedding: &[f32]) -> Observation {
        Observation {
            x,
            y: 0.2,
            width: 0.1,
            height: 0.15,
            confidence: 0.9,
            embedding: embedding.to_vec(),
            person: None,
            suggested: None,
            smile: None,
            eyes_open: None,
        }
    }

    fn identity(path: &str) -> FileIdentity {
        FileIdentity {
            path: PathBuf::from(path),
            file_size: 1000,
            modified_at: 10,
        }
    }

    /// A chip of a stretched crop is a chip of a stretched face. The frame
    /// is not square, so a square in fractions is not a square in pixels.
    #[test]
    fn a_face_thumbnail_is_square_in_pixels_and_not_in_fractions() {
        let frame = (6000u32, 4000u32);
        let (_, _, width, height) = crop((0.4, 0.4, 0.1, 0.1), 0.0, frame);
        let pixels = (width * f64::from(frame.0), height * f64::from(frame.1));
        assert!(
            (pixels.0 - pixels.1).abs() < 1.0,
            "{pixels:?} is not square"
        );
    }

    #[test]
    fn the_margin_keeps_the_hair_and_the_chin() {
        let frame = (1000u32, 1000u32);
        let tight = crop((0.4, 0.4, 0.1, 0.1), 0.0, frame);
        let loose = crop((0.4, 0.4, 0.1, 0.1), 0.35, frame);
        assert!(loose.2 > tight.2, "{loose:?} vs {tight:?}");
        // And it stays centred on the same face.
        assert!(((loose.0 + loose.2 / 2.0) - (tight.0 + tight.2 / 2.0)).abs() < 1e-9);
    }

    /// A face at the very edge of a photograph is where a crop that hangs
    /// over the border shows a smeared edge pixel instead of a cheek.
    #[test]
    fn a_crop_never_leaves_the_photograph() {
        let frame = (1000u32, 1000u32);
        for face in [(0.0, 0.0, 0.1, 0.1), (0.9, 0.9, 0.1, 0.1)] {
            let (x, y, width, height) = crop(face, 0.5, frame);
            assert!(x >= 0.0 && y >= 0.0, "{x},{y}");
            assert!(x + width <= 1.0 + 1e-9, "{x} + {width}");
            assert!(y + height <= 1.0 + 1e-9, "{y} + {height}");
        }
    }

    /// A face larger than the frame with its margin is the ordinary case of
    /// a close portrait, and it must come back as the whole photograph
    /// rather than as something inside out.
    #[test]
    fn a_face_that_fills_the_frame_gives_the_whole_frame() {
        let (x, y, width, height) = crop((0.0, 0.0, 1.0, 1.0), 0.35, (1000, 1000));
        assert_eq!((x, y), (0.0, 0.0));
        assert!((width - 1.0).abs() < 1e-9 && (height - 1.0).abs() < 1e-9);
    }

    #[test]
    fn an_embedding_survives_the_trip_through_the_catalogue() {
        let mut catalog = catalog_with(&["/a/one.jpg"]);
        let photo = catalog.id_of(Path::new("/a/one.jpg")).unwrap().unwrap();
        let embedding: Vec<f32> = (0..128).map(|i| i as f32 / 128.0 - 0.5).collect();
        catalog
            .replace_faces(
                photo,
                &identity("/a/one.jpg"),
                &[observation(0.1, &embedding)],
                0,
            )
            .unwrap();

        let faces = catalog.faces_of(photo).unwrap();
        assert_eq!(faces.len(), 1);
        assert_eq!(faces[0].embedding, embedding);
    }

    /// A second scan of one photograph must not double its faces. v1 deleted
    /// first for exactly this reason and so does this.
    #[test]
    fn scanning_twice_leaves_the_faces_it_found_once() {
        let mut catalog = catalog_with(&["/a/one.jpg"]);
        let photo = catalog.id_of(Path::new("/a/one.jpg")).unwrap().unwrap();
        for _ in 0..3 {
            catalog
                .replace_faces(
                    photo,
                    &identity("/a/one.jpg"),
                    &[observation(0.1, &[1.0]), observation(0.5, &[0.0])],
                    0,
                )
                .unwrap();
        }

        assert_eq!(catalog.faces_of(photo).unwrap().len(), 2);
    }

    /// The whole reason a sweep is bearable on a large library.
    #[test]
    fn a_photograph_already_scanned_is_not_scanned_again() {
        let mut catalog = catalog_with(&["/a/one.jpg", "/a/two.jpg"]);
        assert_eq!(
            catalog
                .unscanned_faces(Path::new("/a"), false)
                .unwrap()
                .len(),
            2
        );

        let photo = catalog.id_of(Path::new("/a/one.jpg")).unwrap().unwrap();
        catalog
            .replace_faces(photo, &identity("/a/one.jpg"), &[], 0)
            .unwrap();
        let left = catalog.unscanned_faces(Path::new("/a"), false).unwrap();
        assert_eq!(left.len(), 1);
        assert_eq!(left[0].path, PathBuf::from("/a/two.jpg"));
    }

    /// And the reason it is not merely "have we seen this path before": a
    /// photograph that has been edited holds different faces now.
    #[test]
    fn a_changed_photograph_is_scanned_again() {
        let mut catalog = catalog_with(&["/a/one.jpg"]);
        let photo = catalog.id_of(Path::new("/a/one.jpg")).unwrap().unwrap();
        catalog
            .replace_faces(photo, &identity("/a/one.jpg"), &[], 0)
            .unwrap();
        assert!(
            catalog
                .unscanned_faces(Path::new("/a"), false)
                .unwrap()
                .is_empty()
        );

        catalog
            .upsert(&NewPhoto {
                path: PathBuf::from("/a/one.jpg"),
                file_size: 2000,
                modified_at: 99,
                taken_at: None,
                width: None,
                height: None,
                orientation: 1,
                camera: None,
                lens: None,
                place: None,
                verdict: Verdict::Nowhere,
                reason: None,
            })
            .unwrap();
        assert_eq!(
            catalog
                .unscanned_faces(Path::new("/a"), false)
                .unwrap()
                .len(),
            1
        );
    }

    /// v1 keyed faces by path, so a rename lost every one of them. This is
    /// the test that says it cannot happen here.
    #[test]
    fn a_renamed_photograph_keeps_its_faces_and_its_people() {
        let mut catalog = catalog_with(&["/a/one.jpg"]);
        let photo = catalog.id_of(Path::new("/a/one.jpg")).unwrap().unwrap();
        catalog
            .replace_faces(
                photo,
                &identity("/a/one.jpg"),
                &[observation(0.1, &[1.0])],
                0,
            )
            .unwrap();
        let person = catalog.person_named("Jana").unwrap();
        let faces: Vec<i64> = catalog
            .faces_of(photo)
            .unwrap()
            .iter()
            .map(|f| f.id)
            .collect();
        catalog.assign_faces(&faces, Some(person)).unwrap();

        catalog
            .moved(Path::new("/a/one.jpg"), Path::new("/a/holiday.jpg"))
            .unwrap();

        let after = catalog.faces_of(photo).unwrap();
        assert_eq!(after.len(), 1);
        assert_eq!(after[0].person, Some(person));
        assert_eq!(after[0].path, PathBuf::from("/a/holiday.jpg"));
    }

    #[test]
    fn a_forgotten_photograph_takes_its_faces_with_it() {
        let mut catalog = catalog_with(&["/a/one.jpg"]);
        let photo = catalog.id_of(Path::new("/a/one.jpg")).unwrap().unwrap();
        catalog
            .replace_faces(
                photo,
                &identity("/a/one.jpg"),
                &[observation(0.1, &[1.0])],
                0,
            )
            .unwrap();
        catalog.forget(&[PathBuf::from("/a/one.jpg")]).unwrap();
        assert!(catalog.faces_of(photo).unwrap().is_empty());
    }

    #[test]
    fn one_name_is_one_person_whatever_the_case() {
        let mut catalog = catalog_with(&[]);
        let first = catalog.person_named("Jana").unwrap();
        assert_eq!(catalog.person_named("jana").unwrap(), first);
        assert_eq!(catalog.person_named("  JANA  ").unwrap(), first);
        assert_eq!(catalog.people().unwrap().len(), 1);
    }

    #[test]
    fn a_person_needs_a_name() {
        let mut catalog = catalog_with(&[]);
        assert!(catalog.person_named("   ").is_err());
    }

    /// Naming a face has to settle it: both named and still waiting to be
    /// asked about would put it in the People window twice.
    #[test]
    fn naming_a_face_clears_the_suggestion_on_it() {
        let mut catalog = catalog_with(&["/a/one.jpg"]);
        let photo = catalog.id_of(Path::new("/a/one.jpg")).unwrap().unwrap();
        let person = catalog.person_named("Jana").unwrap();
        let mut suggested = observation(0.1, &[1.0]);
        suggested.suggested = Some(person);
        catalog
            .replace_faces(photo, &identity("/a/one.jpg"), &[suggested], 0)
            .unwrap();
        assert_eq!(catalog.suggested_faces(64).unwrap().len(), 1);

        let faces: Vec<i64> = catalog
            .faces_of(photo)
            .unwrap()
            .iter()
            .map(|f| f.id)
            .collect();
        let touched = catalog.assign_faces(&faces, Some(person)).unwrap();

        assert_eq!(touched, vec![photo]);
        assert!(catalog.suggested_faces(64).unwrap().is_empty());
        assert!(catalog.unnamed_faces(64).unwrap().is_empty());
        assert_eq!(catalog.faces_of(photo).unwrap()[0].person, Some(person));
    }

    /// The whole of "name this group", checked end to end: the faces take
    /// the name, the photographs take the keyword, and the photographs are
    /// queued to be written.
    #[test]
    fn naming_a_group_puts_the_name_into_the_photographs_too() {
        let mut catalog = catalog_with(&["/a/one.jpg", "/a/two.jpg"]);
        let mut faces = Vec::new();
        for path in ["/a/one.jpg", "/a/two.jpg"] {
            let photo = catalog.id_of(Path::new(path)).unwrap().unwrap();
            catalog
                .replace_faces(photo, &identity(path), &[observation(0.1, &[1.0])], 0)
                .unwrap();
            faces.push(catalog.faces_of(photo).unwrap()[0].id);
        }

        let person = catalog.person_named("Jana").unwrap();
        let touched = catalog.name_faces(&faces, person, 100).unwrap();

        assert_eq!(touched.len(), 2);
        for photo in &touched {
            assert_eq!(catalog.keywords_of(*photo).unwrap(), ["Jana"]);
        }

        assert_eq!(
            catalog.outbox().unwrap().0,
            2,
            "nothing was queued to write"
        );
    }

    /// Two faces of one person on a group shot is ordinary. Removing one
    /// wrong match must not take their name off a photograph they are
    /// plainly still in.
    #[test]
    fn a_name_only_leaves_a_photograph_when_the_person_really_has() {
        let mut catalog = catalog_with(&["/a/group.jpg"]);
        let photo = catalog.id_of(Path::new("/a/group.jpg")).unwrap().unwrap();
        catalog
            .replace_faces(
                photo,
                &identity("/a/group.jpg"),
                &[observation(0.1, &[1.0]), observation(0.5, &[1.0])],
                0,
            )
            .unwrap();
        let person = catalog.person_named("Jana").unwrap();
        let faces: Vec<i64> = catalog
            .faces_of(photo)
            .unwrap()
            .iter()
            .map(|face| face.id)
            .collect();
        catalog.name_faces(&faces, person, 0).unwrap();

        catalog.unname_faces(&faces[..1], 0).unwrap();
        assert_eq!(
            catalog.keywords_of(photo).unwrap(),
            ["Jana"],
            "the name went with the first of two faces"
        );

        catalog.unname_faces(&faces[1..], 0).unwrap();
        assert!(catalog.keywords_of(photo).unwrap().is_empty());
    }

    /// A hand-written tag holds the name on the photograph on its own, so
    /// taking a face off must not undo it.
    #[test]
    fn a_hand_written_tag_keeps_the_name_when_the_face_goes() {
        let mut catalog = catalog_with(&["/a/one.jpg"]);
        let photo = catalog.id_of(Path::new("/a/one.jpg")).unwrap().unwrap();
        catalog
            .replace_faces(
                photo,
                &identity("/a/one.jpg"),
                &[observation(0.1, &[1.0])],
                0,
            )
            .unwrap();
        let person = catalog.person_named("Jana").unwrap();
        let face = catalog.faces_of(photo).unwrap()[0].id;
        catalog.name_faces(&[face], person, 0).unwrap();
        catalog.tag_person(photo, person, 0).unwrap();

        catalog.unname_faces(&[face], 0).unwrap();
        assert_eq!(catalog.keywords_of(photo).unwrap(), ["Jana"]);
    }

    /// Somebody said by hand to be on a photograph is on it as far as any
    /// other program is concerned too.
    #[test]
    fn a_hand_written_tag_puts_the_name_into_the_photograph() {
        let mut catalog = catalog_with(&["/a/one.jpg"]);
        let photo = catalog.id_of(Path::new("/a/one.jpg")).unwrap().unwrap();
        let person = catalog.person_named("Jana").unwrap();
        catalog.tag_person(photo, person, 0).unwrap();
        assert_eq!(catalog.keywords_of(photo).unwrap(), ["Jana"]);

        catalog.untag_person(photo, person, 0).unwrap();
        assert!(catalog.keywords_of(photo).unwrap().is_empty());
    }

    #[test]
    fn a_stranger_waved_away_stops_being_offered() {
        let mut catalog = catalog_with(&["/a/one.jpg"]);
        let photo = catalog.id_of(Path::new("/a/one.jpg")).unwrap().unwrap();
        catalog
            .replace_faces(
                photo,
                &identity("/a/one.jpg"),
                &[observation(0.1, &[1.0])],
                0,
            )
            .unwrap();
        let faces: Vec<i64> = catalog
            .faces_of(photo)
            .unwrap()
            .iter()
            .map(|f| f.id)
            .collect();
        catalog.ignore_faces(&faces).unwrap();
        assert!(catalog.unnamed_faces(64).unwrap().is_empty());

        // And naming it brings it straight back out of that pile.
        let person = catalog.person_named("Jana").unwrap();
        catalog.assign_faces(&faces, Some(person)).unwrap();
        assert_eq!(catalog.faces_of(photo).unwrap()[0].person, Some(person));
    }

    #[test]
    fn forgetting_a_person_returns_their_faces_rather_than_deleting_them() {
        let mut catalog = catalog_with(&["/a/one.jpg"]);
        let photo = catalog.id_of(Path::new("/a/one.jpg")).unwrap().unwrap();
        catalog
            .replace_faces(
                photo,
                &identity("/a/one.jpg"),
                &[observation(0.1, &[1.0])],
                0,
            )
            .unwrap();
        let person = catalog.person_named("Jana").unwrap();
        let faces: Vec<i64> = catalog
            .faces_of(photo)
            .unwrap()
            .iter()
            .map(|f| f.id)
            .collect();
        catalog.assign_faces(&faces, Some(person)).unwrap();

        catalog.delete_person(person).unwrap();
        assert!(catalog.people().unwrap().is_empty());
        assert_eq!(catalog.unnamed_faces(64).unwrap().len(), 1);
    }

    #[test]
    fn a_person_can_be_on_a_photograph_without_a_face_on_it() {
        let mut catalog = catalog_with(&["/a/one.jpg"]);
        let photo = catalog.id_of(Path::new("/a/one.jpg")).unwrap().unwrap();
        let person = catalog.person_named("Jana").unwrap();
        catalog.tag_person(photo, person, 0).unwrap();
        catalog.tag_person(photo, person, 0).unwrap();

        let people = catalog.people_by_photo(Path::new("/a"), false).unwrap();
        assert_eq!(people[&photo].len(), 1);
        assert_eq!(people[&photo][0].name, "Jana");

        catalog.untag_person(photo, person, 0).unwrap();
        assert!(
            catalog
                .people_by_photo(Path::new("/a"), false)
                .unwrap()
                .is_empty()
        );
    }

    /// Taking a person off a photograph is the mirror of naming them: their
    /// face on it goes back to being nobody's.
    #[test]
    fn untagging_returns_that_photographs_faces_to_the_unnamed_pool() {
        let mut catalog = catalog_with(&["/a/one.jpg", "/a/two.jpg"]);
        let one = catalog.id_of(Path::new("/a/one.jpg")).unwrap().unwrap();
        let two = catalog.id_of(Path::new("/a/two.jpg")).unwrap().unwrap();
        let person = catalog.person_named("Jana").unwrap();
        for photo in [one, two] {
            catalog
                .replace_faces(
                    photo,
                    &identity("/a/one.jpg"),
                    &[observation(0.1, &[1.0])],
                    0,
                )
                .unwrap();
            let faces: Vec<i64> = catalog
                .faces_of(photo)
                .unwrap()
                .iter()
                .map(|f| f.id)
                .collect();
            catalog.assign_faces(&faces, Some(person)).unwrap();
        }

        catalog.untag_person(one, person, 0).unwrap();
        assert_eq!(catalog.faces_of(one).unwrap()[0].person, None);
        assert_eq!(catalog.faces_of(two).unwrap()[0].person, Some(person));
    }

    #[test]
    fn the_regions_carry_the_names_the_catalogue_holds_now() {
        let mut catalog = catalog_with(&["/a/one.jpg"]);
        let photo = catalog.id_of(Path::new("/a/one.jpg")).unwrap().unwrap();
        catalog
            .replace_faces(
                photo,
                &identity("/a/one.jpg"),
                &[observation(0.1, &[1.0]), observation(0.5, &[0.0])],
                0,
            )
            .unwrap();
        let faces: Vec<i64> = catalog
            .faces_of(photo)
            .unwrap()
            .iter()
            .map(|f| f.id)
            .collect();
        let person = catalog.person_named("Jana").unwrap();
        catalog.assign_faces(&faces[..1], Some(person)).unwrap();

        let regions = catalog.regions_of(photo).unwrap().unwrap();
        assert_eq!(regions.len(), 1, "only named faces become regions");
        assert_eq!(regions[0].name, "Jana");

        // A rename has to reach the file, which it does because the regions
        // are rebuilt rather than remembered.
        catalog.rename_person(person, "Jana Nova").unwrap();
        assert_eq!(
            catalog.regions_of(photo).unwrap().unwrap()[0].name,
            "Jana Nova"
        );
    }

    /// The difference that keeps us from destroying somebody else's face
    /// frames on the way past.
    #[test]
    fn a_photograph_nobody_has_swept_has_no_answer_about_its_faces() {
        let mut catalog = catalog_with(&["/a/one.jpg"]);
        let photo = catalog.id_of(Path::new("/a/one.jpg")).unwrap().unwrap();
        assert_eq!(catalog.regions_of(photo).unwrap(), None);

        catalog
            .replace_faces(photo, &identity("/a/one.jpg"), &[], 0)
            .unwrap();
        assert_eq!(catalog.regions_of(photo).unwrap(), Some(Vec::new()));
    }

    #[test]
    fn the_expression_tally_counts_only_what_was_looked_at() {
        let mut catalog = catalog_with(&["/a/one.jpg"]);
        let photo = catalog.id_of(Path::new("/a/one.jpg")).unwrap().unwrap();
        let mut scored = observation(0.1, &[1.0]);
        scored.smile = Some(0.9);
        scored.eyes_open = Some(0.1);
        catalog
            .replace_faces(
                photo,
                &identity("/a/one.jpg"),
                &[scored, observation(0.5, &[0.0])],
                0,
            )
            .unwrap();

        let expressions = catalog
            .expressions_by_photo(Path::new("/a"), false)
            .unwrap();
        let summary = expressions[&photo];
        assert_eq!(summary.faces, 2);
        assert_eq!(summary.scored, 1);
        assert_eq!(summary.smiling, 1);
        assert_eq!(summary.eyes_open, 0);
        assert!(summary.anyone_blinking());
        assert!(!summary.anyone_not_smiling());
        assert!(!summary.all_smiling(), "one face was never looked at");
    }

    #[test]
    fn scoring_an_old_face_keeps_its_number_and_its_person() {
        let mut catalog = catalog_with(&["/a/one.jpg"]);
        let photo = catalog.id_of(Path::new("/a/one.jpg")).unwrap().unwrap();
        catalog
            .replace_faces(
                photo,
                &identity("/a/one.jpg"),
                &[observation(0.1, &[1.0])],
                0,
            )
            .unwrap();
        let person = catalog.person_named("Jana").unwrap();
        let face = catalog.faces_of(photo).unwrap()[0].id;
        catalog.assign_faces(&[face], Some(person)).unwrap();

        assert_eq!(
            catalog
                .missing_expressions(Path::new("/a"), false)
                .unwrap()
                .len(),
            1
        );
        catalog
            .score_expressions(&[(face, Some(0.8), Some(0.9))])
            .unwrap();

        let after = catalog.faces_of(photo).unwrap();
        assert_eq!(after[0].id, face);
        assert_eq!(after[0].person, Some(person));
        assert_eq!(after[0].smile, Some(0.8));
        assert!(
            catalog
                .missing_expressions(Path::new("/a"), false)
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn the_average_of_a_persons_faces_comes_back_per_person() {
        let mut catalog = catalog_with(&["/a/one.jpg", "/a/two.jpg"]);
        let mut people = Vec::new();
        for (index, path) in ["/a/one.jpg", "/a/two.jpg"].iter().enumerate() {
            let photo = catalog.id_of(Path::new(path)).unwrap().unwrap();
            catalog
                .replace_faces(
                    photo,
                    &identity(path),
                    &[observation(0.1, &[index as f32, 1.0])],
                    0,
                )
                .unwrap();
            let person = catalog.person_named(&format!("Person {index}")).unwrap();
            let face = catalog.faces_of(photo).unwrap()[0].id;
            catalog.assign_faces(&[face], Some(person)).unwrap();
            people.push(person);
        }

        let embeddings = catalog.person_embeddings().unwrap();
        assert_eq!(embeddings.len(), 2);
        assert!(embeddings.iter().all(|(_, faces)| faces.len() == 1));
    }

    #[test]
    fn a_recursive_folder_collects_what_is_underneath_it() {
        let separator = std::path::MAIN_SEPARATOR;
        let root = format!("{separator}a");
        let deep = format!("{root}{separator}trip{separator}one.jpg");
        let mut catalog = catalog_with(&[&deep]);
        let photo = catalog.id_of(Path::new(&deep)).unwrap().unwrap();
        let person = catalog.person_named("Jana").unwrap();
        catalog.tag_person(photo, person, 0).unwrap();

        assert!(
            catalog
                .people_by_photo(Path::new(&root), false)
                .unwrap()
                .is_empty()
        );
        assert_eq!(
            catalog
                .people_by_photo(Path::new(&root), true)
                .unwrap()
                .len(),
            1
        );
    }
}
