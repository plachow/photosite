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

## Processes

**Scan** — indexing a folder into the catalogue. Incremental: an unchanged file
is recognised by length, write time and metadata reader version.

**Plan** — the full list of destination paths a batch or import would write,
computed before anything is written so a summary can be shown and the user can
still change their mind. `BatchPlan`, `ImportPlan`.

**Preset** — a named, stored set of batch or export settings. Presets describe
what to do, not where the user currently is; a preset with no output folder
keeps whatever destination is already on screen.

**Outbox** — the transactional queue of metadata changes waiting to be written
into files by exiftool. The catalogue is updated and the outbox row inserted in
one transaction, so a crash cannot lose a rating.

**Face scan** (`FaceEngine`, `PeopleDialog`) — finding the faces in a folder
with the local YuNet detector and describing each with an SFace embedding
(OpenCV Zoo models under `tools/models`; nothing leaves the machine). Faces
live only in the catalogue (`faces`, `people`, `face_scans`); unnamed ones are
grouped by embedding similarity for bulk naming, and naming a group writes the
person's name into each photograph's keywords through the outbox. During a
scan, a face that clearly matches an already-named person (cosine ≥ 0.5) is
assigned automatically; unchanged files are skipped, so the sweep is
incremental. Rejected synonyms: *face tagging*, *people detection*.

**Describe** (`OllamaVisionService`, `AiTagDialog`) — asking a vision model on
a local Ollama server to fill a photograph's title, description and keywords.
The model receives a downscaled preview and must answer a fixed JSON schema.
Results flow through the same catalogue-and-outbox path as a manual edit;
keywords merge into the existing list, and in fill-empty mode a photo already
carrying both a title and a description is skipped, which is what makes an
interrupted bulk run restartable. When the run's language is not English, the
same call also returns an **English description**, stored in the catalogue
only (`description_en`) so search works in both languages — the file always
carries just the primary-language description, and a re-scan never touches
the English one. Rejected synonyms: *auto-tag*, *caption*.

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
