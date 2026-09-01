# PhotoSite domain

The vocabulary the code uses. Prefer these terms; the rejected synonyms below
appear in other photo software and mean something subtly different here.

## Core concepts

**Recipe** (`EditRecipe`) — the complete non-destructive description of an edit
to one photograph: geometry, adjustments, filters and layers. A recipe never
contains pixels and is never applied to the file on disk until an explicit
save, export or batch run. Rejected synonyms: *edit*, *settings*, *develop
settings*.

**Adjustments** (`PhotoAdjustments`) — the photographic sliders inside a
recipe: tone, colour, levels, curves, detail, lens. Every value is neutral at
its default, so `PhotoAdjustments.Neutral` is `new()`. Not to be confused with
**filters**.

**Filter** (`FilterStep`) — a creative effect appended to a recipe as a list
step: sharpen, blur, pixelize, sepia and so on. Filters stack in order; the
same filter can appear twice. Adjustments do not stack — there is one set.

**Layer** (`AnnotationLayer`) — a vector object above the photograph: shape,
text or freehand. Layers are positioned in the coordinate space of the
finished, oriented, cropped image, which is the space the exporter composes
into. They stay editable until export; nothing is ever rasterized on creation.

**Crop region** (`CropRegion`) — a rectangle normalized to 0..1 of the source
frame. Used both for the recipe's crop and for a transient canvas selection.

**Record** (`PhotoRecord`) — one immutable row of the catalogue: the file's
identity, its indexed metadata, and the organisation applied to it.

**Tab** (`MainViewModel.EditorTabs`) — the strip above the window: one Manager
tab plus one tab per open editor. A tab owns its photo's editor session, undo
history included, so switching to Manager or to another tab never prompts to
save; only closing a tab (its ✕, or Esc inside the editor) ends the session
and asks about unsaved edits. Paging to the next photo inside the editor
retargets the current tab rather than opening new ones. Rejected synonyms:
*document*, *workspace*.

## Organisation

**Rating** — 0 to 5 stars. Written into the file (XMP and EXIF).

**Colour label** — one of five named labels, stored as its XMP name so it
survives a round-trip through other software. Written into the file.

**Flag** — the culling verdict: picked, none, or rejected. Deliberately *not*
written into the file: it is a working state for one culling session. A
rejected photo stays on disk; deleting is always a separate step.

**Keywords** — a `;`-separated list, written to `dc:subject` and IPTC.

**Approximate location** (`PhotoRecord.HasApproximateLocation`) — the file's
own GPS evidence says the coordinates are probably off: the receiver wrote a
horizontal error estimate of 100 m or more (`GPSHPositioningError`), its fix
was already two minutes stale when the shutter fired — the "quickly pull the
phone out" photo that stamps a location from hundreds of metres back along
the walk — or `GPSProcessingMethod` admits the position never came from
satellites at all (CELLID/WLAN/NETWORK; phones re-stamp such a cached
network position for hours). The readings are taken at scan time
(`gps_error_meters`, `gps_fix_age_seconds`, `gps_processing_method`,
`gps_altitude`); the fix age compares `DateTimeOriginal` with the UTC GPS
stamp, trusting `OffsetTimeOriginal` when present and otherwise assuming the
nearest quarter-hour timezone. A cell fix with no real altitude (a receiver
with an actual fix knows its height; a tower estimate writes zero) is a pure
tower guess, typically kilometres off. A second threshold (500 m, a fix ten
minutes stale, or a pure cell-tower fix) grades the position "probably far
off", and any network-sourced position grades at least approximate; the
info panel's Map button carries the verdict as a green/amber/red dot
(`LocationAccuracy`) and disables without coordinates, while the 📍≈
thumbnail badge and the filter's Location facet treat both graded tiers as
approximate. Hand-typed
coordinates clear the evidence in the catalogue and retire the file's error
and stamp tags through the outbox, so a corrected photo stops reading as
approximate; sidecar coordinates never carry evidence at all. Rejected
synonyms: *bad GPS*, *GPS accuracy*.

## Processes

**Scan** — indexing a folder into the catalogue. Incremental: an unchanged file
is recognised by length, write time and metadata reader version.

**Plan** — the full list of destination paths a batch would write, computed
before anything is written so a summary can be shown and the user can still
change their mind. Two collisions are decided in it and they are different:
a name already on disk, and a name another photograph in the same plan is
about to take. v2: `batch::Plan`, `batch::plan`. v1: `BatchPlan`,
`ImportPlan`.

**Preset** — a named, stored set of batch or export settings, kept in the
catalogue as text. Presets describe what to do, not where the user currently
is. The starter set is handed out once ever: one deleted stays deleted. v2:
`batch::Preset`, the `presets` table.

**Outbox** — the transactional queue of photographs waiting to be written
into. The catalogue is updated and the outbox row inserted in one
transaction, so a crash cannot lose a rating. v1 queued the *change* and had
exiftool write it; v2 queues the *photograph* and writes what the catalogue
says at the moment of writing, itself, so a retry cannot write something
since undone and the queue cannot disagree with the catalogue.

**Face scan** (v2: `photosite-faces`, `faces::sweep`, the People window; v1:
`FaceEngine`, `PeopleDialog`) — finding the faces in a folder with the local
YuNet detector and describing each with an SFace embedding (OpenCV Zoo
models; v2 keeps them in `models` beside the catalogue, v1 under
`tools/models`; nothing leaves the machine). Faces live in the catalogue
(`faces`, `people`, `photo_people`, `face_scans`) keyed by the photograph's
**number** rather than its path, so a rename carries them and a deleted
photograph takes them with it; unnamed ones are grouped by embedding
similarity for bulk naming, and naming a group writes the person's name into
each photograph's keywords through the outbox. During a scan, a face that
clearly matches an already-named person (cosine ≥ 0.5) is assigned
automatically; unchanged files are skipped, so the sweep is incremental.
Rejected synonyms: *face tagging*, *people detection*.

**Suggestion** — a face whose best match falls between SFace's same-identity
boundary (cosine 0.363) and the auto-assign threshold (0.5). It waits in
neither the person nor the unnamed pool until the user answers yes or no in
the People window; only a yes writes the name anywhere. Deciding a face either
way always clears its suggestion.

**Expression** (v2: `people::Expressions`, `people::SMILE_THRESHOLD`; v1:
`FaceExpression`, `ExpressionSummary`) — two 0..1 scores the face scan
attaches to every face: *smile* (FER+ happiness on the aligned crop) and
*eyes open* (open-closed-eye-0001 per eye, the face keeps the weaker eye).
Both models are optional files beside the other two; a face scanned without
them stays unscored (`NULL`) and is scored in place on a later scan by
rectangle overlap, never losing its id or person. Scores stay in the
catalogue only — nothing is written into files. Per photo they aggregate to
one summary (thresholds at 0.5) behind the thumbnail badge, the info panel's
Expression row — which counts, `1/2 smiling · 2/2 eyes open`, rather than
passing a verdict — and the filter's Smile and Eyes facets, whose sides read
"everyone passes" versus "someone fails" so a portrait cull can keep either
pile. Rejected synonyms: *mood*, *emotion detection*.

**Region** — a named face rectangle written into the file as an MWG region
(`XMP-mwg-rs`, centre-based normalized areas plus the pixel dimensions), the
format Lightroom, digiKam and Windows read face frames from. v2 writes them
itself through the ordinary XMP merge (`meta::xmp::Regions`), with no
exiftool; they are rebuilt from the catalogue at the moment of writing, and
*not knowing* is distinguished from *nobody is named* so that a photograph
nobody has swept keeps whatever frames another program left in it. Being
rebuilt is what carries a rename into the files, while a removed person's
frames follow the same leave-what-was-written policy as keywords only when
nothing rewrites that photo again. The gallery can filter by person — the
People chips are the one facet that composes by conjunction, because a
photograph has any number of people — and the preview frames the named
faces, each in that person's own colour (v1 framed the unnamed and the
suggested ones too, in grey and amber). Each person keeps one stable colour
(v2: `theme::person_color`; v1: `PersonBrushes`) keyed by id, across
thumbnail badges, filter chips and the frames over the preview. Removing a
face from a person is the mirror of naming: the face returns to the unnamed
pool, the keyword comes back out of that photo when no other face of the
person remains on it, and the regions are rebuilt.

**Describe** (v2: `photosite-ai`, the Describe window; v1:
`OllamaVisionService`, `AiTagDialog`) — asking a vision model on a local
Ollama server to fill a photograph's title, description and keywords. The
model receives a downscaled preview and must answer a fixed JSON schema.
Results flow through the same catalogue-and-outbox path as a manual edit;
keywords merge into the existing list, and in fill-empty mode a photo
already carrying both a title and a description is skipped, which is what
makes an interrupted bulk run restartable. When the run's language is not
English, the same call also returns an **English description**, stored in
the catalogue only (`description_en`) so search works in both languages —
the file always carries just the primary-language description, and a re-scan
never touches the English one. A photo with coordinates is first
reverse-geocoded offline — v2 through a GeoNames extract held in memory
(`gazetteer::Gazetteer`, a one-degree grid), v1 through the database bundled
with exiftool (`ExifToolGeolocator`, one run per ~100 m grid cell, cached) —
and the model receives the **verified place** as text — "in or near X", or
"about N km east of X" when the nearest catalogued place is far — never raw
coordinates, which a local model would confidently mis-geocode. The resolved
names also lead the keyword list deterministically (most specific first);
the nearest place drops out of the keywords beyond 10 km or on an
approximate-location fix, which softens the prompt to "probably" instead.
Rejected synonyms: *auto-tag*, *caption*.

**Surface** — a rendered bitmap the editor canvas paints: straightening,
adjustments and filters applied, but crop, orientation and layers deliberately
left out because the canvas expresses those as cheap transforms it can change
every frame.

## Invariants

- `ImageRenderer` is the only path from a recipe to pixels. The viewer, Save
  As, export and batch all go through it, which is what guarantees an exported
  file matches its preview.
- A re-scan never overwrites organisation the user applied inside PhotoSite
  unless the file itself changed.
- Records and recipes are value types by behaviour: recipes compare their list
  members element-wise, because undo and dirty tracking depend on it.
- Nothing that touches the file system or decodes pixels runs on the
  dispatcher.
