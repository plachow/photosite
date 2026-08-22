# PhotoSite

PhotoSite is a Windows-first photo manager and editor built around one idea:
nothing is written to your photographs until you say so. Browsing, rating,
organising and editing all happen against a non-destructive recipe; the
original file is only touched by an explicit save, export or batch run.

## Manager

- recursive, cancellable folder indexing with a persistent SQLite catalogue,
  warm-start loading and live folder watching;
- back / forward / up navigation with a clickable breadcrumb;
- a lazy Windows folder tree that restores the last selected folder;
- a virtualized thumbnail grid that only realizes visible tiles, plus a
  compact details list;
- thumbnail size slider (or `Ctrl`+wheel), with cached thumbnails generated in
  the background at a resolution matched to the tile size;
- sort by date taken, file name, rating, date modified, file size or
  dimensions, in either direction;
- filter by rating, colour label, pick/reject flag, orientation, file format,
  camera, lens and capture date, offering only the values present in the
  folder;
- search across file names, titles, descriptions and keywords;
- 0–5 star ratings, five colour labels, pick and reject flags, and keywords -
  all applied to the whole selection at once;
- an information panel with capture date, camera, lens, the exposure triangle,
  dimensions, file size and GPS with an **open in map** action, and in-place
  editing of title, description and keywords;
- Explorer-style file operations: copy, move, rename, duplicate, delete to the
  Recycle Bin, create folder, and reveal in File Explorer.

### Comparison and culling

`Ctrl+K` opens two to four selected photographs side by side with synchronized
pan and zoom, a 100 % match button, and the same rating and pick/reject keys as
the gallery. `Tab` moves the focus, `Delete` drops a photo from the comparison.

Rejected photos stay on disk and in the catalogue - they simply dim in the
gallery, and can be hidden entirely from the filter panel. Deleting is always a
separate, explicit step.

### Import

`Ctrl+I` imports from a camera or memory card, which is detected automatically
when it has a `DCIM` folder. Photos can be organised into dated folders,
renamed from their capture time, and written to a second location as a backup
before the primary copy. Files already in the destination are recognised by
name, length and a hash of their first and last blocks, so re-inserting the
same card imports only what is new.

### Batch conversion

`Ctrl+B` runs a batch over the selection:

- resize by width, height, longest side, shortest side or percentage, always
  preserving the aspect ratio and never enlarging unless asked;
- convert to JPEG, PNG, WebP, TIFF or BMP with a quality setting;
- rename with a prefix, suffix, custom or date-based base name and sequential
  numbering;
- keep, strip, or keep-but-anonymize metadata;
- optional output sharpening and a subfolder per date;
- choose what happens to existing files: add a number, skip, or overwrite.

Every destination path is computed before anything is written, so the dialog
states exactly how many files will be written, skipped or overwritten and shows
the first real output path while the settings are still being typed.

Settings are saved as named presets. **Facebook export**, **Web gallery**,
**Original quality JPEG** and **Small email photos** are provided once on first
use; delete one and it stays deleted.

### AI description

**Describe with AI…** in the gallery context menu sends each selected
photograph to a vision model running on a local [Ollama](https://ollama.com)
server and fills in its title, description and keywords, in Czech or English.
The model sees a downscaled preview, answers a fixed JSON schema, and the
results are written through the catalogue and the metadata outbox exactly like
a manual edit - into the database and into the photo files themselves.
Keywords are merged with the ones already on the photo; by default only empty
titles and descriptions are filled, and photos that already carry both are
skipped, so an interrupted overnight run can simply be started again over the
same selection. When the language is not English, the same call also returns
an English description that is kept in the catalogue only, so search finds
photos in either language while the file carries just the primary one. During
a run the dialog shows the average pace and the projected finish time, and it
ends with a summary of how many photos were described, how fast, and when.
Nothing ever leaves the machine.

## Editor

The editor renders the whole recipe live. Crop, rotation and flips stay cheap
transforms, while adjustments, filters and straightening are rendered off the
UI thread and coalesced, so dragging a slider stays smooth.

- **Light and colour** - exposure, contrast, highlights, shadows, whites,
  blacks, brightness, clarity, temperature, tint, vibrance, saturation, black
  and white points, midpoint and gamma;
- **Auto Fix** measures the frame - percentiles, mean luminance, colour cast,
  average saturation - and applies a deliberately conservative correction. It
  writes to the ordinary sliders, so every decision it made is visible and can
  be retuned or undone. It always measures the unadjusted frame, so pressing it
  twice gives the same answer;
- **Auto white balance** and an **eyedropper** that solves for the temperature
  and tint which neutralize the patch you click;
- **Histogram** of exactly what is on the canvas, per channel or combined, with
  shadow and highlight clipping warnings;
- **Detail** - sharpening with radius and threshold, luminance and colour noise
  reduction, vignette, devignetting, distortion and fringing correction;
- **Geometry** - straighten and keystone correction, which scale the frame up
  just enough that no empty corner is left behind;
- **Crop** - free, original, 1:1, 4:3, 3:2, 16:9, 3:4 and 2:3, held while
  dragging any handle;
- **Filters** - sharpen, unsharp mask, blur, Gaussian blur, pixelize, noise
  reduction, add noise, grayscale, sepia and vignette, each with a live preview
  and a before toggle;
- **Layers** - arrows, lines, rectangles, ellipses, text and freehand drawing
  stay editable vector objects until export. They reorder, duplicate, hide and
  delete, and every step takes part in undo;
- **Before / after** as a straight toggle or a split canvas;
- **Export** with resize, format, quality, metadata handling and its own preset
  list.

Adjustments are folded into a single lookup table per channel before the first
pixel is touched, so a full-resolution render is a matter of milliseconds per
megapixel rather than seconds.

## RAW

RAW files browse, preview, open and export. Where Windows has a codec for the
camera - a vendor codec or Microsoft's Raw Image Extension - it is used. Where
it does not, the camera's own full-size preview is extracted from the file
instead, so a folder straight off a card is never a grid of grey tiles. Ratings,
labels and keywords for RAW files are written to an `.xmp` sidecar.

## Shortcuts

### Manager

| Key | Action |
| --- | --- |
| `1`–`5`, `` ` `` | Rating, clear rating |
| `6`–`9`, `0` | Colour label, clear label |
| `P` / `X` | Pick / reject |
| `Alt`+`←` `→` `↑` | Back, forward, up one folder |
| `F5` | Refresh |
| `F2` | Rename |
| `Ctrl+A` | Select all |
| `Ctrl+C` / `Ctrl+V` | Copy / paste the physical files |
| `Alt+C` / `Alt+X` | Copy or move to a chosen folder |
| `Ctrl+Shift+C` | Copy to the last destination |
| `Ctrl+B` | Batch convert |
| `Ctrl+I` | Import |
| `Ctrl+K` | Compare |
| `Del` | Move to the Recycle Bin |
| `F` | Fullscreen |
| `Enter` | Open in the editor |

### Editor

| Key | Action |
| --- | --- |
| `Ctrl+Z` / `Ctrl+Y` | Undo / redo |
| `Ctrl+Shift+S` | Save as |
| `Ctrl+E` | Export |
| `Ctrl+U` | Upload to Imgur and copy the URL |
| `Ctrl+V` | Open a clipboard bitmap as a new unsaved image |
| `B` | Show the original |
| `R` | Rotate |
| `C` | Crop selection; `Enter` applies it |
| `V` `A` `L` `S` `O` `T` `D` | Select, arrow, line, rectangle, ellipse, text, draw |
| `Del` | Delete the selected layer |
| `Esc` | Back to the manager |

Shortcuts never fire while a text field has focus.

## Build

Requirements:

- Windows 10 1809 or newer;
- .NET 10 SDK.

```powershell
dotnet build PhotoSite.slnx
dotnet run --project tests/PhotoSite.SmokeTests/PhotoSite.SmokeTests.csproj
dotnet run --project src/PhotoSite.App/PhotoSite.App.csproj
```

Runtime data is stored under `%LOCALAPPDATA%\PhotoSite`. Source photographs
remain unchanged unless you explicitly choose **Overwrite original**, run a
batch that writes over them, or import with the delete-originals option.

Pass a photo to open it directly in the editor, or pass a directory to open it
in the manager:

```powershell
PhotoSite.exe "C:\Photos\portrait.jpg"
PhotoSite.exe "C:\Photos"
```

A directly opened photo behaves like a lightweight viewer: `Esc` closes the
window and `Enter` opens the manager in the photo's directory.

## Releases and automatic updates

Production releases are created from semantic version tags. The release
workflow builds and smoke-tests the application, publishes a self-contained
`win-x64` build, packages it with Velopack, and uploads the installer and update
feed to GitHub Releases.

```powershell
git tag v0.1.0
git push origin v0.1.0
```

Install `PhotoSite-Setup.exe` from the first GitHub Release once. Installed
copies then check the stable release feed after startup, download newer versions
in the background, and ask before restarting. Unsaved editor changes keep the
same save/discard/cancel protection during an update restart.

The update client accesses GitHub Releases anonymously. The release repository
must therefore be publicly readable; never embed a GitHub access token in the
desktop application. If the source repository remains private, publish the
Velopack artifacts to a separate public release repository or an HTTPS static
file host and update `AppUpdateService.ReleaseRepository`.

Release artifacts are not code-signed yet. Configure Velopack's
`--signTemplate` with a trusted RSA code-signing certificate before distributing
PhotoSite outside a controlled test group.

## Architecture

The WPF surface sits on top of small, testable services:

- `Services/Imaging` is the pixel engine. `ImageRenderer` is the single path
  from a recipe to pixels, shared by the viewer, Save As, export and batch, so
  an exported file always matches its preview. `AdjustmentPipeline` folds every
  per-channel operation into one 3×256 lookup table and only runs the
  operations that genuinely need neighbouring channels per pixel.
- `Domain/EditRecipe` is the complete non-destructive description of an edit:
  geometry, adjustments, filters and vector layers. It compares its list
  members by value, which is what makes undo and dirty tracking correct.
- `Services/Batch` plans every destination before writing and reports progress
  off the UI thread.
- `Infrastructure/PhotoCatalogRepository` owns the SQLite schema and migrates
  it in place, so an existing catalogue keeps its ratings, recipes and
  thumbnails.
- Durable metadata is written through a transactional outbox drained by
  `MetadataOutboxProcessor` into exiftool, with XMP sidecars for RAW.

Work still planned:

- Vortice Direct3D 11/DXGI/Direct2D render surface;
- prioritized prefetch of adjacent photographs;
- ICC monitor-profile conversion;
- tiled rendering for images larger than a GPU texture;
- rendering only the visible region while zoomed to 100 %, so adjusting a
  slider at full resolution costs no more than at fit-to-window;
- an RGB curve editor on top of the curve model the pipeline already applies;
- lens-profile database for automatic distortion and vignetting correction.
