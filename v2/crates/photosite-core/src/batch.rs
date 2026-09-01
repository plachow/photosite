//! Converting a lot of photographs at once, and knowing what will happen
//! before it does.
//!
//! **Every destination is worked out before a single byte is written.** That
//! is the whole design, and it is not tidiness: a batch is the one thing in
//! this application that can quietly destroy somebody's work, and a plan is
//! what makes the destruction visible while there is still time to change
//! one's mind. The dialog can say *forty written, three overwritten* because
//! the answer already exists — and none of the deciding touches a disk, so a
//! test can watch all of it.
//!
//! Two collisions matter and they are different. A name already **on disk**
//! is one; a name another photograph **in this same plan** is about to take
//! is the other. v1 learned the second one the hard way: two photographs
//! from different folders, both called `DSC_0042`, converted into one
//! folder, and the second silently replaced the first. So the plan claims
//! each name as it goes.

use crate::domain::Photo;
use crate::transfer::free_name;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

/// What the output is written as.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum Format {
    /// Whatever the source already was. For a batch that only resizes or
    /// renames, and the reason it is not simply "JPEG" — converting a PNG
    /// screenshot to JPEG because somebody wanted it smaller is a decision
    /// nobody asked for.
    Same,
    #[default]
    Jpeg,
    Png,
    WebP,
    Tiff,
    Bmp,
}

impl Format {
    pub const ALL: [Self; 6] = [
        Self::Same,
        Self::Jpeg,
        Self::Png,
        Self::WebP,
        Self::Tiff,
        Self::Bmp,
    ];

    /// The extension an output takes. `Same` keeps the source's, and falls
    /// back to JPEG for a file that has none.
    pub fn extension(self, source: Option<&str>) -> String {
        match self {
            Self::Same => source
                .filter(|extension| !extension.is_empty())
                .map(str::to_ascii_lowercase)
                .unwrap_or_else(|| "jpg".to_owned()),
            Self::Jpeg => "jpg".to_owned(),
            Self::Png => "png".to_owned(),
            Self::WebP => "webp".to_owned(),
            Self::Tiff => "tif".to_owned(),
            Self::Bmp => "bmp".to_owned(),
        }
    }

    /// Does a quality setting mean anything here?
    ///
    /// PNG, TIFF and BMP are lossless, and so — here — is WebP: the only
    /// pure-Rust encoder there is writes lossless, and a lossy one would
    /// mean shipping libwebp. A quality slider over any of them is a control
    /// that does nothing, which is worse than no control.
    pub fn has_quality(self) -> bool {
        matches!(self, Self::Jpeg | Self::Same)
    }

    /// Something a person needs to know before choosing this, or nothing.
    ///
    /// A lossless WebP of a photograph is larger than a quality-85 JPEG, not
    /// smaller. Somebody choosing WebP for a web gallery is choosing it for
    /// the opposite of what they will get, so the dialog says so where the
    /// quality slider would have been.
    pub fn note_key(self) -> Option<&'static str> {
        matches!(self, Self::WebP).then_some("format-webp-lossless")
    }

    pub fn title_key(self) -> &'static str {
        match self {
            Self::Same => "format-same",
            Self::Jpeg => "format-jpeg",
            Self::Png => "format-png",
            Self::WebP => "format-webp",
            Self::Tiff => "format-tiff",
            Self::Bmp => "format-bmp",
        }
    }
}

/// Which measurement the size applies to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum Resize {
    #[default]
    None,
    Width,
    Height,
    /// The longer of the two, whichever it is. What "make these 2048 pixels"
    /// means to almost everybody, because it treats an upright frame and a
    /// wide one the same.
    LongestSide,
    ShortestSide,
    /// A percentage of what it already is.
    Percent,
}

impl Resize {
    pub const ALL: [Self; 5] = [
        Self::None,
        Self::LongestSide,
        Self::ShortestSide,
        Self::Width,
        Self::Height,
    ];

    pub fn title_key(self) -> &'static str {
        match self {
            Self::None => "resize-none",
            Self::Width => "resize-width",
            Self::Height => "resize-height",
            Self::LongestSide => "resize-longest",
            Self::ShortestSide => "resize-shortest",
            Self::Percent => "resize-percent",
        }
    }
}

/// Where the output's name comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum Naming {
    #[default]
    Original,
    /// One word for all of them, which only makes sense with numbering —
    /// and the plan makes sure of it rather than trusting anybody to
    /// remember.
    Custom,
    DateTaken,
}

impl Naming {
    pub const ALL: [Self; 3] = [Self::Original, Self::Custom, Self::DateTaken];

    pub fn title_key(self) -> &'static str {
        match self {
            Self::Original => "naming-original",
            Self::Custom => "naming-custom",
            Self::DateTaken => "naming-date",
        }
    }
}

/// What to do about a name that is already in use.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum OnCollision {
    /// Take a number instead. The only one of the three that cannot lose
    /// anything, which is why it is the default.
    #[default]
    Number,
    Skip,
    Overwrite,
}

impl OnCollision {
    pub const ALL: [Self; 3] = [Self::Number, Self::Skip, Self::Overwrite];

    pub fn title_key(self) -> &'static str {
        match self {
            Self::Number => "collision-number",
            Self::Skip => "collision-skip",
            Self::Overwrite => "collision-overwrite",
        }
    }
}

/// What of the original's metadata the output carries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum Carry {
    /// Everything the original said about itself: when, with what, at what
    /// exposure, and everything anybody has said here.
    #[default]
    Everything,
    /// Nothing at all. For a photograph going somewhere public where the
    /// camera's serial number is nobody's business.
    Nothing,
    /// Everything except where it was taken. The one somebody wants for a
    /// photograph of their house.
    WithoutPlace,
}

impl Carry {
    pub const ALL: [Self; 3] = [Self::Everything, Self::WithoutPlace, Self::Nothing];

    pub fn title_key(self) -> &'static str {
        match self {
            Self::Everything => "carry-everything",
            Self::Nothing => "carry-nothing",
            Self::WithoutPlace => "carry-without-place",
        }
    }
}

/// A named set of batch settings.
///
/// The same shape backs "export this one photograph" and "convert these
/// hundred and fifty", so the two cannot drift apart — which they will, the
/// moment they are two structures.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Preset {
    pub name: String,

    // Where it goes.
    /// `None` with `beside_source` off means the batch cannot run, and the
    /// plan says so per photograph rather than refusing as a whole.
    pub into: Option<String>,
    /// Beside each original instead of into one folder.
    pub beside_source: bool,
    /// A folder per day, under the destination.
    pub folder_per_day: bool,
    pub on_collision: OnCollision,

    // What it is.
    pub format: Format,
    /// 1..=100. Meaningless for the lossless formats, and the dialog hides
    /// it there rather than showing a control that does nothing.
    pub quality: u8,

    // What size.
    pub resize: Resize,
    pub resize_to: u32,
    /// Whether a photograph smaller than the target is blown up. Off, and
    /// deliberately: "make these 2048 wide" almost never means "and invent
    /// pixels for the small ones".
    pub allow_enlarging: bool,
    /// Sharpening after the downscale, 0..=100. Nought is off.
    pub sharpen: u8,

    // What it is called.
    pub naming: Naming,
    pub custom_name: String,
    /// Tokens, not a format language: `{year} {month} {day} {hour} {minute}
    /// {second}`. The same idea as the map address, and for the same reason
    /// — a pattern language we half-implement is worse than none.
    pub date_format: String,
    pub prefix: String,
    pub suffix: String,
    pub numbering: bool,
    pub number_from: u32,
    pub number_digits: u8,
    pub number_separator: String,

    pub carry: Carry,
}

impl Default for Preset {
    fn default() -> Self {
        Self {
            name: String::new(),
            into: None,
            beside_source: false,
            folder_per_day: false,
            on_collision: OnCollision::Number,
            format: Format::Jpeg,
            quality: 88,
            resize: Resize::None,
            resize_to: 2048,
            allow_enlarging: false,
            sharpen: 0,
            naming: Naming::Original,
            custom_name: String::new(),
            date_format: "{year}-{month}-{day}_{hour}{minute}{second}".to_owned(),
            prefix: String::new(),
            suffix: String::new(),
            numbering: false,
            number_from: 1,
            number_digits: 3,
            number_separator: "_".to_owned(),
            carry: Carry::Everything,
        }
    }
}

impl Preset {
    /// The sets offered on a fresh installation.
    ///
    /// Ordinary presets, every one of them: deleting "For the web" leaves it
    /// deleted, because they are written into the catalogue once and never
    /// put back. A starter set that reappears is a starter set nobody can
    /// get rid of.
    pub fn starters() -> Vec<Self> {
        vec![
            Self {
                name: "For sharing".to_owned(),
                format: Format::Jpeg,
                quality: 85,
                resize: Resize::LongestSide,
                resize_to: 2048,
                sharpen: 35,
                // The one place a default takes something away, and it is
                // the right way round: a photograph going somewhere public
                // need not carry the street it was taken in.
                carry: Carry::WithoutPlace,
                suffix: "_share".to_owned(),
                ..Default::default()
            },
            Self {
                name: "For the web".to_owned(),
                // JPEG and not WebP, though v1 offered WebP here: what this
                // application can write is lossless WebP, which for a
                // photograph is larger than the JPEG it would replace. A
                // starter preset that quietly does the opposite of its name
                // is worse than one fewer starter preset.
                format: Format::Jpeg,
                quality: 82,
                resize: Resize::LongestSide,
                resize_to: 1600,
                sharpen: 30,
                carry: Carry::Nothing,
                ..Default::default()
            },
            Self {
                name: "Full size JPEG".to_owned(),
                format: Format::Jpeg,
                quality: 97,
                ..Default::default()
            },
            Self {
                name: "Small enough to email".to_owned(),
                format: Format::Jpeg,
                quality: 78,
                resize: Resize::LongestSide,
                resize_to: 1200,
                sharpen: 40,
                carry: Carry::Nothing,
                suffix: "_small".to_owned(),
                ..Default::default()
            },
        ]
    }
}

/// Which list a preset belongs to.
///
/// One shape, two lists: what somebody wants for a hundred photographs and
/// what they want for one they are exporting are different sets of numbers,
/// and mixing them makes both lists useless.
pub const BATCH: &str = "batch";
pub const EXPORT: &str = "export";

/// The setting that records that the starters have been handed out.
const SEEDED: &str = "presets.seeded";

impl crate::catalog::Catalog {
    /// Every preset of one kind, in order, with the starters handed out the
    /// first time and never again.
    ///
    /// **Never again** is the whole of it. Somebody who deletes "For the
    /// web" should not find it back the next time they start the
    /// application; a starter set that reappears is one nobody can get rid
    /// of.
    pub fn presets(&self, kind: &str) -> anyhow::Result<Vec<Preset>> {
        let mut statement = self.connection().prepare(
            "SELECT name, settings FROM presets WHERE kind = ?1 ORDER BY name COLLATE NOCASE",
        )?;
        let rows = statement
            .query_map(rusqlite::params![kind], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;

        let mut presets = Vec::with_capacity(rows.len());
        for (name, settings) in rows {
            match toml::from_str::<Preset>(&settings) {
                // The name is the key, so it wins over whatever the text
                // says: renaming one is a change to one column.
                Ok(preset) => presets.push(Preset { name, ..preset }),
                // One unreadable preset must not hide the rest of the list.
                Err(error) => tracing::warn!(%name, %error, "a preset cannot be read"),
            }
        }

        Ok(presets)
    }

    /// Hands out the starter presets, once ever.
    pub fn seed_presets(&mut self) -> anyhow::Result<()> {
        if self.setting(SEEDED)?.is_some() {
            return Ok(());
        }

        for preset in Preset::starters() {
            self.save_preset(BATCH, &preset)?;
        }

        self.set_setting(SEEDED, "yes")?;
        Ok(())
    }

    pub fn save_preset(&self, kind: &str, preset: &Preset) -> anyhow::Result<()> {
        let name = preset.name.trim();
        anyhow::ensure!(!name.is_empty(), "a preset needs a name");
        self.connection().execute(
            "INSERT INTO presets(kind, name, settings) VALUES(?1, ?2, ?3)
             ON CONFLICT(kind, name) DO UPDATE SET settings = excluded.settings",
            rusqlite::params![kind, name, toml::to_string(preset)?],
        )?;
        Ok(())
    }

    pub fn delete_preset(&self, kind: &str, name: &str) -> anyhow::Result<()> {
        self.connection().execute(
            "DELETE FROM presets WHERE kind = ?1 AND name = ?2",
            rusqlite::params![kind, name],
        )?;
        Ok(())
    }
}

/// Why a photograph is not going to be written.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Skipped {
    /// No destination folder was chosen.
    Nowhere,
    /// The name is taken and the preset says leave it alone.
    Exists,
}

impl Skipped {
    pub fn title_key(self) -> &'static str {
        match self {
            Self::Nowhere => "batch-nowhere",
            Self::Exists => "batch-exists",
        }
    }
}

/// One photograph's place in the plan.
#[derive(Debug, Clone, PartialEq)]
pub struct Step {
    pub from: PathBuf,
    pub to: PathBuf,
    /// The output would replace a file that is already there. Only ever true
    /// when the preset asked for that.
    pub overwrites: bool,
    pub skipped: Option<Skipped>,
}

/// Everything a batch would do.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Plan {
    pub steps: Vec<Step>,
}

impl Plan {
    pub fn writes(&self) -> usize {
        self.steps
            .iter()
            .filter(|step| step.skipped.is_none())
            .count()
    }

    pub fn skips(&self) -> usize {
        self.steps
            .iter()
            .filter(|step| step.skipped.is_some())
            .count()
    }

    pub fn overwrites(&self) -> usize {
        self.steps
            .iter()
            .filter(|step| step.skipped.is_none() && step.overwrites)
            .count()
    }

    /// The first real destination, so a dialog can show where things will
    /// land while the settings are still being typed.
    pub fn example(&self) -> Option<&Path> {
        self.steps
            .iter()
            .find(|step| step.skipped.is_none())
            .map(|step| step.to.as_path())
    }
}

/// Works out every destination, touching nothing.
///
/// `exists` answers whether a path is already on disk. It is a parameter and
/// not a call to the file system, because the whole point of this function
/// is that it can be tested — and because a plan built against a fake disk
/// is how the collision rules were checked at all.
pub fn plan(photos: &[Photo], preset: &Preset, exists: &dyn Fn(&Path) -> bool) -> Plan {
    let mut steps = Vec::with_capacity(photos.len());
    // Names this plan has already promised to somebody. Without it two
    // photographs of the same name land on each other and the second wins.
    let mut claimed: BTreeSet<PathBuf> = BTreeSet::new();
    let mut number = preset.number_from;

    for photo in photos {
        let Some(folder) = folder_for(photo, preset) else {
            steps.push(Step {
                from: photo.path.clone(),
                to: PathBuf::new(),
                overwrites: false,
                skipped: Some(Skipped::Nowhere),
            });
            continue;
        };

        let wanted = folder.join(file_name(photo, preset, number));
        number = number.saturating_add(1);

        let on_disk = exists(&wanted);
        let taken = on_disk || claimed.contains(&wanted);
        // Writing over the photograph we are reading is never an accident
        // worth allowing: the read happens first, but a half-written output
        // over the original is a photograph gone.
        let over_the_source = same_file(&wanted, &photo.path);

        let (to, overwrites, skipped) = match preset.on_collision {
            _ if !taken && !over_the_source => (wanted, false, None),
            OnCollision::Skip => (wanted, true, Some(Skipped::Exists)),
            OnCollision::Overwrite if !over_the_source => (wanted, true, None),
            // Overwrite or Number, both landing on the source itself: take
            // a number. "Overwrite" means the files that were already
            // there, not the one being read.
            _ => {
                let free = free_name(&wanted, &|path| exists(path) || claimed.contains(path));
                (free, false, None)
            }
        };

        claimed.insert(to.clone());
        steps.push(Step {
            from: photo.path.clone(),
            to,
            overwrites,
            skipped,
        });
    }

    Plan { steps }
}

/// Which folder a photograph's output goes into.
fn folder_for(photo: &Photo, preset: &Preset) -> Option<PathBuf> {
    let root = if preset.beside_source {
        photo.path.parent().map(Path::to_path_buf)?
    } else {
        let into = preset.into.as_deref()?.trim().to_owned();
        if into.is_empty() {
            return None;
        }

        PathBuf::from(into)
    };

    if !preset.folder_per_day {
        return Some(root);
    }

    // A photograph with no date goes in the root rather than into a folder
    // called "unknown": a folder nobody asked for is worse than a file in
    // the obvious place.
    match photo.taken_at {
        Some(taken) => Some(root.join(crate::time::format_date(taken))),
        None => Some(root),
    }
}

/// The output's file name.
pub fn file_name(photo: &Photo, preset: &Preset, number: u32) -> String {
    let original = photo
        .path
        .file_stem()
        .map(|stem| stem.to_string_lossy().into_owned())
        .unwrap_or_default();

    let stem = match preset.naming {
        Naming::Original => original.clone(),
        Naming::Custom => tidy(&preset.custom_name),
        Naming::DateTaken => match photo.taken_at {
            Some(taken) => tidy(&fill_in(&preset.date_format, taken)),
            // No date to name it by. Its own name is a better answer than
            // today's, which is a lie about when it was taken.
            None => original.clone(),
        },
    };

    // Whatever was asked for came out empty — an all-punctuation custom
    // name, a date template with no tokens in it. The original name is the
    // one thing that is always there.
    let stem = if stem.trim().is_empty() {
        original
    } else {
        stem
    };

    let mut name = format!("{}{stem}{}", tidy(&preset.prefix), tidy(&preset.suffix));
    if preset.numbering {
        let digits = preset.number_digits.clamp(1, 9) as usize;
        name.push_str(&tidy(&preset.number_separator));
        name.push_str(&format!("{number:0digits$}"));
    }

    let extension = preset.format.extension(
        photo
            .path
            .extension()
            .and_then(|extension| extension.to_str()),
    );
    format!("{name}.{extension}")
}

/// The date tokens filled in.
///
/// Unknown tokens are left standing rather than emptied, so a typo shows as
/// itself in the name instead of quietly producing `2022--_.jpg`.
fn fill_in(template: &str, taken: i64) -> String {
    let (year, month, day, hour, minute, second) = crate::time::civil(taken);
    template
        .replace("{year}", &format!("{year:04}"))
        .replace("{month}", &format!("{month:02}"))
        .replace("{day}", &format!("{day:02}"))
        .replace("{hour}", &format!("{hour:02}"))
        .replace("{minute}", &format!("{minute:02}"))
        .replace("{second}", &format!("{second:02}"))
}

/// Takes out what a file name may not hold.
///
/// Windows refuses these outright; the others accept some of them and then
/// nobody can delete the file from a terminal. A name typed into a box has
/// to be turned into one that can exist, not into an error message.
fn tidy(text: &str) -> String {
    text.chars()
        .filter(|c| {
            !matches!(c, '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*') && !c.is_control()
        })
        .collect()
}

/// Are these two the same file? Compared as text, without asking the disk:
/// the plan is built without touching one.
fn same_file(left: &Path, right: &Path) -> bool {
    let flat = |path: &Path| path.to_string_lossy().to_lowercase().replace('/', "\\");
    flat(left) == flat(right)
}

/// The output size, or nothing when the photograph is left at its own.
///
/// The aspect ratio is always kept: a batch that squashed a frame to fit a
/// number would be a batch nobody could use for anything.
pub fn measure(width: u32, height: u32, preset: &Preset) -> Option<(u32, u32)> {
    if preset.resize == Resize::None || width == 0 || height == 0 || preset.resize_to == 0 {
        return None;
    }

    let to = f64::from(preset.resize_to);
    let (width_f, height_f) = (f64::from(width), f64::from(height));
    let scale = match preset.resize {
        Resize::None => return None,
        Resize::Width => to / width_f,
        Resize::Height => to / height_f,
        Resize::LongestSide => to / width_f.max(height_f),
        Resize::ShortestSide => to / width_f.min(height_f),
        Resize::Percent => to / 100.0,
    };

    if !scale.is_finite() || scale <= 0.0 {
        return None;
    }

    if scale > 1.0 && !preset.allow_enlarging {
        return None;
    }

    let target = (
        ((width_f * scale).round() as u32).max(1),
        ((height_f * scale).round() as u32).max(1),
    );
    (target != (width, height)).then_some(target)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{Organisation, PhotoId};
    use crate::place::Verdict;

    fn photo(path: &str) -> Photo {
        Photo {
            id: PhotoId(1),
            path: PathBuf::from(path),
            folder: PathBuf::from(path).parent().unwrap().to_path_buf(),
            file_size: 1000,
            modified_at: 0,
            taken_at: Some(1_655_300_000),
            width: Some(6000),
            height: Some(4000),
            orientation: 1,
            camera: None,
            lens: None,
            organisation: Organisation::default(),
            people: Vec::new(),
            expressions: Default::default(),
            place: None,
            verdict: Verdict::Nowhere,
            reason: None,
        }
    }

    fn nothing(_: &Path) -> bool {
        false
    }

    fn into(folder: &str) -> Preset {
        Preset {
            into: Some(folder.to_owned()),
            ..Default::default()
        }
    }

    #[test]
    fn a_plan_names_every_photograph_before_anything_is_written() {
        let photos = [photo("/a/one.jpg"), photo("/a/two.png")];
        let plan = plan(&photos, &into("/out"), &nothing);
        assert_eq!(plan.writes(), 2);
        assert_eq!(plan.skips(), 0);
        assert_eq!(plan.steps[0].to, PathBuf::from("/out").join("one.jpg"));
        assert_eq!(plan.steps[1].to, PathBuf::from("/out").join("two.jpg"));
    }

    #[test]
    fn keeping_the_format_keeps_the_extension() {
        let preset = Preset {
            format: Format::Same,
            ..into("/out")
        };
        let plan = plan(&[photo("/a/two.PNG")], &preset, &nothing);
        assert_eq!(plan.steps[0].to, PathBuf::from("/out").join("two.png"));
    }

    /// The one v1 learned the hard way: two photographs of the same name
    /// from different folders, converted into one place.
    #[test]
    fn two_photographs_of_one_name_do_not_land_on_each_other() {
        let photos = [photo("/a/DSC_0042.jpg"), photo("/b/DSC_0042.jpg")];
        let plan = plan(&photos, &into("/out"), &nothing);
        assert_ne!(plan.steps[0].to, plan.steps[1].to);
        assert_eq!(plan.writes(), 2);
    }

    #[test]
    fn a_name_already_on_disk_takes_a_number() {
        let taken = |path: &Path| path == PathBuf::from("/out").join("one.jpg");
        let plan = plan(&[photo("/a/one.jpg")], &into("/out"), &taken);
        assert_eq!(plan.steps[0].to, PathBuf::from("/out").join("one (2).jpg"));
        assert!(!plan.steps[0].overwrites);
    }

    #[test]
    fn skipping_says_so_rather_than_writing_somewhere_else() {
        let taken = |path: &Path| path == PathBuf::from("/out").join("one.jpg");
        let preset = Preset {
            on_collision: OnCollision::Skip,
            ..into("/out")
        };
        let plan = plan(&[photo("/a/one.jpg")], &preset, &taken);
        assert_eq!(plan.skips(), 1);
        assert_eq!(plan.steps[0].skipped, Some(Skipped::Exists));
    }

    #[test]
    fn overwriting_is_counted_so_it_can_be_shown_before_it_happens() {
        let taken = |path: &Path| path == PathBuf::from("/out").join("one.jpg");
        let preset = Preset {
            on_collision: OnCollision::Overwrite,
            ..into("/out")
        };
        let plan = plan(&[photo("/a/one.jpg")], &preset, &taken);
        assert_eq!(plan.overwrites(), 1);
        assert_eq!(plan.writes(), 1);
    }

    /// "Overwrite" means the files already at the destination, never the
    /// photograph being read. Writing over the source mid-convert is a
    /// photograph gone.
    #[test]
    fn a_batch_never_writes_over_the_photograph_it_is_reading() {
        let here = |path: &Path| path == Path::new("/a/one.jpg");
        for on_collision in OnCollision::ALL {
            let preset = Preset {
                beside_source: true,
                format: Format::Same,
                on_collision,
                ..Default::default()
            };
            let plan = plan(&[photo("/a/one.jpg")], &preset, &here);
            let step = &plan.steps[0];
            if step.skipped.is_none() {
                assert_ne!(
                    step.to,
                    PathBuf::from("/a/one.jpg"),
                    "{on_collision:?} wrote over the source"
                );
            }
        }
    }

    #[test]
    fn without_anywhere_to_put_them_the_plan_says_so_rather_than_failing() {
        let plan = plan(&[photo("/a/one.jpg")], &Preset::default(), &nothing);
        assert_eq!(plan.skips(), 1);
        assert_eq!(plan.steps[0].skipped, Some(Skipped::Nowhere));
        assert!(plan.example().is_none());
    }

    #[test]
    fn beside_the_source_means_beside_each_source() {
        let preset = Preset {
            beside_source: true,
            suffix: "_x".to_owned(),
            ..Default::default()
        };
        let photos = [photo("/a/one.jpg"), photo("/b/two.jpg")];
        let plan = plan(&photos, &preset, &nothing);
        assert_eq!(plan.steps[0].to, PathBuf::from("/a/one_x.jpg"));
        assert_eq!(plan.steps[1].to, PathBuf::from("/b/two_x.jpg"));
    }

    #[test]
    fn a_folder_per_day_puts_them_under_the_day_they_were_taken() {
        let preset = Preset {
            folder_per_day: true,
            ..into("/out")
        };
        let plan = plan(&[photo("/a/one.jpg")], &preset, &nothing);
        let expected = PathBuf::from("/out")
            .join(crate::time::format_date(1_655_300_000))
            .join("one.jpg");
        assert_eq!(plan.steps[0].to, expected);
    }

    #[test]
    fn a_photograph_with_no_date_is_not_put_in_a_folder_called_nothing() {
        let mut undated = photo("/a/one.jpg");
        undated.taken_at = None;
        let preset = Preset {
            folder_per_day: true,
            ..into("/out")
        };
        let plan = plan(&[undated], &preset, &nothing);
        assert_eq!(plan.steps[0].to, PathBuf::from("/out").join("one.jpg"));
    }

    #[test]
    fn numbering_runs_across_the_whole_batch() {
        let preset = Preset {
            naming: Naming::Custom,
            custom_name: "holiday".to_owned(),
            numbering: true,
            number_from: 8,
            number_digits: 3,
            ..into("/out")
        };
        let photos = [photo("/a/one.jpg"), photo("/a/two.jpg")];
        let plan = plan(&photos, &preset, &nothing);
        assert_eq!(plan.steps[0].to.file_name().unwrap(), "holiday_008.jpg");
        assert_eq!(plan.steps[1].to.file_name().unwrap(), "holiday_009.jpg");
    }

    /// One name for a hundred photographs without numbering is ninety-nine
    /// collisions. They must not become ninety-nine lost files.
    #[test]
    fn one_name_without_numbering_still_produces_a_hundred_files() {
        let preset = Preset {
            naming: Naming::Custom,
            custom_name: "holiday".to_owned(),
            ..into("/out")
        };
        let photos: Vec<Photo> = (0..5).map(|n| photo(&format!("/a/{n}.jpg"))).collect();
        let plan = plan(&photos, &preset, &nothing);
        let names: BTreeSet<&Path> = plan.steps.iter().map(|step| step.to.as_path()).collect();
        assert_eq!(names.len(), 5, "some of them landed on each other");
    }

    #[test]
    fn a_date_name_is_built_from_tokens() {
        let preset = Preset {
            naming: Naming::DateTaken,
            date_format: "{year}{month}{day}-{hour}{minute}".to_owned(),
            ..into("/out")
        };
        let plan = plan(&[photo("/a/one.jpg")], &preset, &nothing);
        let name = plan.steps[0].to.file_name().unwrap().to_string_lossy();
        let (year, month, day, hour, minute, _) = crate::time::civil(1_655_300_000);
        assert_eq!(
            name,
            format!("{year:04}{month:02}{day:02}-{hour:02}{minute:02}.jpg")
        );
    }

    /// A date name for a photograph with no date has to be something. Its
    /// own name is the honest answer; today's date is a lie about when it
    /// was taken.
    #[test]
    fn a_date_name_falls_back_to_the_original_rather_than_to_today() {
        let mut undated = photo("/a/holiday.jpg");
        undated.taken_at = None;
        let preset = Preset {
            naming: Naming::DateTaken,
            ..into("/out")
        };
        let plan = plan(&[undated], &preset, &nothing);
        assert_eq!(plan.steps[0].to.file_name().unwrap(), "holiday.jpg");
    }

    #[test]
    fn a_name_that_cannot_exist_is_made_into_one_that_can() {
        let preset = Preset {
            naming: Naming::Custom,
            custom_name: "a/b:c*d".to_owned(),
            ..into("/out")
        };
        let plan = plan(&[photo("/a/one.jpg")], &preset, &nothing);
        assert_eq!(plan.steps[0].to.file_name().unwrap(), "abcd.jpg");
    }

    #[test]
    fn a_name_made_entirely_of_punctuation_falls_back_to_the_original() {
        let preset = Preset {
            naming: Naming::Custom,
            custom_name: "///".to_owned(),
            ..into("/out")
        };
        let plan = plan(&[photo("/a/one.jpg")], &preset, &nothing);
        assert_eq!(plan.steps[0].to.file_name().unwrap(), "one.jpg");
    }

    #[test]
    fn the_dialog_can_show_where_the_first_one_goes() {
        let plan = plan(&[photo("/a/one.jpg")], &into("/out"), &nothing);
        assert_eq!(
            plan.example(),
            Some(PathBuf::from("/out").join("one.jpg").as_path())
        );
    }

    // ------------------------------------------------------------- sizing

    #[test]
    fn the_aspect_ratio_is_always_kept() {
        let preset = Preset {
            resize: Resize::LongestSide,
            resize_to: 3000,
            ..Default::default()
        };
        let (width, height) = measure(6000, 4000, &preset).unwrap();
        assert_eq!((width, height), (3000, 2000));
    }

    #[test]
    fn the_longest_side_is_the_longest_side_whichever_it_is() {
        let preset = Preset {
            resize: Resize::LongestSide,
            resize_to: 2000,
            ..Default::default()
        };
        assert_eq!(measure(4000, 6000, &preset), Some((1333, 2000)));
        assert_eq!(measure(6000, 4000, &preset), Some((2000, 1333)));
    }

    #[test]
    fn the_shortest_side_is_the_other_one() {
        let preset = Preset {
            resize: Resize::ShortestSide,
            resize_to: 2000,
            ..Default::default()
        };
        assert_eq!(measure(6000, 4000, &preset), Some((3000, 2000)));
    }

    /// "Make these 2048 wide" almost never means "and invent pixels for the
    /// small ones".
    #[test]
    fn a_small_photograph_is_not_blown_up_unless_asked() {
        let mut preset = Preset {
            resize: Resize::LongestSide,
            resize_to: 4000,
            ..Default::default()
        };
        assert_eq!(measure(1000, 800, &preset), None);
        preset.allow_enlarging = true;
        assert_eq!(measure(1000, 800, &preset), Some((4000, 3200)));
    }

    #[test]
    fn a_photograph_already_the_right_size_is_left_alone() {
        let preset = Preset {
            resize: Resize::Width,
            resize_to: 6000,
            ..Default::default()
        };
        assert_eq!(measure(6000, 4000, &preset), None);
    }

    #[test]
    fn nonsense_sizes_do_nothing_rather_than_something_strange() {
        for preset in [
            Preset {
                resize: Resize::LongestSide,
                resize_to: 0,
                ..Default::default()
            },
            Preset {
                resize: Resize::None,
                resize_to: 100,
                ..Default::default()
            },
        ] {
            assert_eq!(measure(6000, 4000, &preset), None);
        }

        assert_eq!(
            measure(
                0,
                0,
                &Preset {
                    resize: Resize::LongestSide,
                    resize_to: 100,
                    ..Default::default()
                }
            ),
            None
        );
    }

    #[test]
    fn a_percentage_is_of_what_it_already_is() {
        let preset = Preset {
            resize: Resize::Percent,
            resize_to: 50,
            ..Default::default()
        };
        assert_eq!(measure(6000, 4000, &preset), Some((3000, 2000)));
    }

    // ------------------------------------------------------------ presets

    #[test]
    fn a_preset_survives_the_trip_through_the_catalogue() {
        for preset in Preset::starters() {
            let text = toml::to_string(&preset).unwrap();
            let back: Preset = toml::from_str(&text).unwrap();
            assert_eq!(back, preset);
        }
    }

    #[test]
    fn the_starters_are_handed_out_once_and_not_again() {
        let mut catalog = crate::Catalog::in_memory().unwrap();
        catalog.seed_presets().unwrap();
        assert_eq!(catalog.presets(BATCH).unwrap().len(), 4);

        // Somebody deletes one, and it stays deleted.
        catalog.delete_preset(BATCH, "For the web").unwrap();
        catalog.seed_presets().unwrap();
        let left = catalog.presets(BATCH).unwrap();
        assert_eq!(left.len(), 3);
        assert!(!left.iter().any(|preset| preset.name == "For the web"));
    }

    #[test]
    fn a_preset_comes_back_as_it_went_in() {
        let catalog = crate::Catalog::in_memory().unwrap();
        let preset = Preset {
            name: "Mine".to_owned(),
            format: Format::WebP,
            quality: 71,
            resize: Resize::ShortestSide,
            resize_to: 900,
            sharpen: 12,
            numbering: true,
            carry: Carry::WithoutPlace,
            ..Default::default()
        };
        catalog.save_preset(BATCH, &preset).unwrap();
        assert_eq!(catalog.presets(BATCH).unwrap(), vec![preset.clone()]);

        // Saving it again replaces it rather than making a second.
        catalog
            .save_preset(
                BATCH,
                &Preset {
                    quality: 40,
                    ..preset
                },
            )
            .unwrap();
        let stored = catalog.presets(BATCH).unwrap();
        assert_eq!(stored.len(), 1);
        assert_eq!(stored[0].quality, 40);
    }

    #[test]
    fn the_two_lists_do_not_see_each_other() {
        let catalog = crate::Catalog::in_memory().unwrap();
        catalog
            .save_preset(
                EXPORT,
                &Preset {
                    name: "One".to_owned(),
                    ..Default::default()
                },
            )
            .unwrap();
        assert!(catalog.presets(BATCH).unwrap().is_empty());
        assert_eq!(catalog.presets(EXPORT).unwrap().len(), 1);
    }

    /// One unreadable preset must not hide the rest of the list.
    #[test]
    fn a_damaged_preset_costs_only_itself() {
        let catalog = crate::Catalog::in_memory().unwrap();
        catalog
            .save_preset(
                BATCH,
                &Preset {
                    name: "Good".to_owned(),
                    ..Default::default()
                },
            )
            .unwrap();
        catalog
            .connection()
            .execute(
                "INSERT INTO presets(kind, name, settings) VALUES('batch', 'Broken', 'not toml = =')",
                [],
            )
            .unwrap();

        let presets = catalog.presets(BATCH).unwrap();
        assert_eq!(presets.len(), 1);
        assert_eq!(presets[0].name, "Good");
    }

    #[test]
    fn a_preset_needs_a_name() {
        let catalog = crate::Catalog::in_memory().unwrap();
        assert!(catalog.save_preset(BATCH, &Preset::default()).is_err());
    }

    #[test]
    fn the_starters_all_have_names_and_are_different() {
        let names: BTreeSet<String> = Preset::starters()
            .into_iter()
            .map(|preset| preset.name)
            .collect();
        assert_eq!(names.len(), Preset::starters().len());
        assert!(names.iter().all(|name| !name.trim().is_empty()));
    }

    /// A quality slider over a lossless format is a control that does
    /// nothing, which is worse than no control.
    #[test]
    fn quality_only_means_something_where_it_does() {
        assert!(Format::Jpeg.has_quality());
        assert!(!Format::WebP.has_quality(), "we write it lossless");
        assert!(!Format::Png.has_quality());
        assert!(!Format::Tiff.has_quality());
        assert!(!Format::Bmp.has_quality());
    }

    /// A format with a surprise in it has to say so where the control that
    /// would have explained it used to be.
    #[test]
    fn a_format_that_will_surprise_somebody_says_so() {
        assert!(Format::WebP.note_key().is_some());
        for format in [Format::Jpeg, Format::Png, Format::Tiff, Format::Bmp] {
            assert!(format.note_key().is_none(), "{format:?}");
        }
    }
}
