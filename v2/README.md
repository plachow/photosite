# PhotoSite v2

A clean sheet. Rust, egui over wgpu, Windows / macOS / Linux from one source.

```bash
cd v2
cargo run --release -p photosite-ui              # the application
cargo run --release -p photosite-cli -- doctor   # where everything lives
cargo test --workspace                           # 276 tests, no window, no GPU
```

## Layout

```
crates/
  photosite-core     domain, catalogue + migrations, paths, settings, log, tasks, commands
  photosite-image    decoding, downscaling, EXIF
  photosite-meta     what a photograph says about itself: XMP and EXIF, read and written
  photosite-ui       egui — the only crate that knows about the GPU
  photosite-cli      headless: runs the whole pipeline without a window
```

**Neither the core nor the image crate may carry `egui`, `eframe`, `wgpu` or
`winit` anywhere in its dependency graph.** The test in
`crates/photosite-core/tests/no_ui.rs` guards it by asking `cargo tree` rather
than the sources — an indirect dependency would never show up in a `use` line.
This is not tidiness for its own sake: losing this boundary is the single
reason the v1 port was expensive.

The CLI exists for the same reason. A CI runner has no screen and no GPU, but
the scan, the catalogue and the migrations still have to be tested.

## Principles worth the trouble

Every one of them came from a specific mistake, not from a handbook.

**Paths can be overridden.** `--data <folder>` or `PHOTOSITE_DATA` redirects
data, settings, cache and log under a single root. In v1 the path was
hard-wired, which meant nothing could be measured or tried out except against
real data.

**A removed setting does not cost the rest of the file.** A key we stopped
using is dropped and noted in the log; only a file we genuinely cannot read is
set aside. Otherwise somebody would lose everything they ever configured, and
the only thing they did wrong was to use the application earlier.

**Nothing fails in silence.** A task that fails carries that in its state and
goes to the log. Damaged settings are set aside, not discarded. A crash leaves
a report. Over one swallowed result, the prototype drew not a single thumbnail
and nowhere was there a word about it. Even the log itself can lie: the filter
lists crates, and for a binary the logging target is the *target* name, not
the package name, so `photosite_ui` matched nothing and not one line from the
application reached the file. Warnings and errors fell through the general
level at the end, so there was no way to see it.

**Background work is not a queue.**
[`Wishlist`](crates/photosite-core/src/jobs.rs) is overwritten every frame
with whatever is visible; whatever drops out of it is never done. A queue meant
seven seconds of blank tiles after the scrollbar was released, because
thousands of dead requests were being waited on.

**Migrations from the first table.** Without them there is no second release.
A catalogue from a newer build refuses to open rather than being damaged.

**A command registry from the start.** Shortcuts, buttons and any future menu
read from one list. Retrofitting it into a finished UI means going through
every button one at a time. Whether a command belongs on the toolbar it says
itself: the drawing layer used to ask by name ("everything in *View* except
`view.recursive`") and such a rule needs rewriting with every command added
after it.

**A native dialog belongs alongside, not in the middle.** The folder dialog is
created on the main thread — macOS cannot pin the panel to the window
otherwise — but it is awaited [on a thread
alongside](crates/photosite-ui/src/picker.rs). Blocking the render for that
time is tempting and it means not one tile is redrawn for as long as somebody
browses a disk; Windows declares such a window unresponsive after a few
seconds. At most one is ever open, or a second Ctrl+O stacks another on top of
the first.

**Writes in batches.** A row per transaction looks innocent; a scan of 7,558
photographs took 44 s that way, and 0.3 s with one transaction per thousand
rows.

## Docks

The panes are not wired side by side in the drawing layer. The layout is a
**tree that is data** — one line in the settings:

```
h(0.16, tree, h(0.66, gallery, v(0.62, preview, info)))
```

A horizontal split gives the first part sixteen percent of the width and the
second whatever is left; in the vertical column on the right, the preview sits
above the photo details. Adding a pane below the preview is therefore a change
to that string, not a change to the drawing —
`crates/photosite-ui/src/docks.rs` knows nothing of any particular layout.

The notation is textual on purpose: it goes into the settings on one line, it
can be corrected by hand, and it shows in a diff at a glance. Nested TOML
tables three levels deep would be unreadable. A nonsensical layout is refused
and the default takes over, whether a bracket is missing, a pane is listed
twice, or there is no grid in it.

**A dock cannot be closed by accident.** Every one has a minimum size and the
splitter will not go below it; hiding is only possible through a command that
can also bring it back. Before that held, the preview pane could be dragged to
zero, it was saved to the settings and nothing brought it back — clicking a
tile still worked, there was simply nowhere to draw.

**Every pane must carry its own key.** egui gives the children of one parent
the same salt (`"child"`) and tells them apart only by the order they were
created in, so two scroll areas reach for one shared state: the wheel over the
folder tree moved the tiles in the grid.
`the_wheel_moves_only_the_pane_under_the_mouse` measures it — with no window
and no GPU, because egui can be run that way too. A second test deliberately
builds the panes without keys and insists that they do run together; a gauge
that can never fail measures nothing.

## Localisation

The default language and the fallback are both **en-US**; whatever is missing
from another translation is taken from English, so a bare key never stays on
screen. The bundle is `crates/photosite-core/i18n/<language>/photosite.ftl`,
and the language can be switched with `--lang cs-CZ` or in the settings.

The format is [Fluent](https://projectfluent.org), and deliberately so: Czech
has three plural forms (*1 fotka, 2 fotky, 5 fotek*) and a format that cannot
do that would later have to be rewritten along with every call site. Number
rounding therefore lives in the bundle too (`NUMBER($ms,
maximumFractionDigits: 0)`) rather than in code — how many decimal places show
is a decision of the language.

**Neither the UI nor the CLI holds a single hard-written string**, and two
tests in `crates/photosite-ui/tests/translations.rs` guard it: one checks that
every key used exists in the bundle, the other walks the sources and reports a
literal handed to a widget. A third test checks the search itself — a gauge
that quietly stops noticing is worse than none.

Neither the command registry nor the theme palettes hold text, only keys.
Adding a language therefore means adding one `.ftl` and one line to
`i18n::available()`.

## Settings

Four principles, every one of them expensive to introduce afterwards:

1. **Defaults live in code, once.** `Default` is the single source of truth.
2. **Only what differs goes into the file.** Saving the whole tree means
   freezing today's defaults for everyone who ever started the application.
   When we change our minds, they get the new ones too.
3. **Everything has a path.** `gallery.tile_size` can be read and written as
   text, so the settings screen is generated from the field descriptions
   (`TUNABLES`) rather than written by hand. Adding an option means adding a
   line.
4. **Reset is first class.** One entry, a whole group, or everything. The
   window position and the last opened folder are not what a reset means —
   they are not preferences.

Ready-made sets of settings are deliberately **not** here. There were some,
and they were premature: one of them literally copied the defaults, so
improving those would have quietly left it on the old ones, and for the rest
there was no telling whether anybody would want them. Reset covers "give me
back something sensible" entirely. Once there are enough settings for
combinations to make sense, they will be data in a file, not constants in
code.

**Not one constant affecting looks or behaviour is left in the UI** — the gap
between tiles, the aspect ratio, the caption height, the uploads per frame,
the texture ceiling, the thread count and the idle interval are all settings.
The test `the_field_descriptions_cover_exactly_what_the_settings_hold` keeps
the description and the reality from drifting apart in either direction.

## Themes

Colours are **data**, not constants in code. Today they are built in (`dark`,
`light`, `grey`, `sepia`, `seabreeze`), but precisely because they are
serialisable they will one day be loadable from a file without touching
anything that draws. `appearance.theme = "automatic"` follows the system, and
`theme_dark` and `theme_light` say which theme belongs to which mode.

The core knows nothing of egui: `Color` is three bytes, and the UI layer does
the conversion.

The palette covers **every** role the drawing layer needs — primary,
secondary and disabled text, accent, warnings, errors, edges. Whatever the
palette does not settle, the toolkit fills in its own way, and its defaults
argue with a foreign palette; that is how dark grey text on a grey background
appears. `override_text_color` is deliberately unused: it would force one
colour on all text and erase the difference between states.

**Legibility is guarded by a test, not by eye.** `every_theme_is_legible`
walks every theme times fourteen foreground-background pairs and measures
contrast by WCAG: 4.5 for primary text, 3.0 for secondary and accent, 2.2 for
disabled. An illegible combination is a failing test, not a report from a
user. Alongside it, `the_bevel_shows_without_shouting` keeps the hint of
relief in the range where it is visible without turning the frame into a
button.

Disabled text is not drawn by the palette but by egui, which blends the text
colour toward `noninteractive.weak_bg_fill`. So the test
`disabled_text_stays_legible_even_after_egui` calls the toolkit's own function
and measures what comes out of it.

One trap that was here and is fixed: `ctx.set_visuals` writes only into the
slot of the theme currently chosen. At startup the system had not yet reported
the mode, so it wrote into the dark one — and the moment "light" arrived, egui
switched to the light slot with its own colours and the settings window glowed
white in the middle of a dark application. It now writes into both.

## Aspect ratio

Everything is drawn at the ratio of the frame; nothing is ever deformed.
Letterboxing yes, stretching no.

One place broke that and it was not visible at a glance: **the EXIF thumbnail
carries whatever ratio suited the camera.** A Nikon stores a 160x120 thumbnail
next to a 6000x4000 file and squeezes the whole scene into it — nothing is
cropped, it is simply narrowed. Tiles are filled from that thumbnail first, so
until the sharp version finished decoding the photograph was twelve percent
wider than it should have been. While scrolling a large library, that is most
of what is on screen.

The thumbnail is therefore rescaled to the ratio of the frame, read from the
SOF marker in the header we hold in memory anyway. Where the ratio already
matches — and phones store it correctly — it is left alone; rescaling would
only blur it.

## The thumbnail cache

The `loading.texture_budget` ceiling is a **wish, not a law**: it must not go
below what is on screen right now. A smaller ceiling does not mean "less
memory" but an endless round — every frame something is evicted, ordered again
at once and decoded again. With 80px tiles, the preview pane closed and a
ceiling of 300, that burned **72% of a core at complete rest** and the tiles
along the edges flickered.

Three things hold it:

* the ceiling is raised to the size of the screen plus a quarter
  (`effective_budget`),
* prefetched rows count as in use, or they are the first to go and are ordered
  again at once,
* once the sharp version arrives, the quick one from EXIF is discarded —
  keeping both is double the pressure for nothing.

And it repaints only when there is something to show: the decoding threads ask
for a frame themselves. The condition "something is still missing" was here
before and it was a trap — when a missing tile could not be filled, the
application span at full speed.

| at rest, 7,558 photographs, 80px tiles | |
|---|---|
| before | 72% of a core |
| after | **0%** |

## Culling

The first of v1's features to come across: **the stars, the colour labels, the
pick and reject verdict, the keywords and the words**, on a whole selection at
once, with `1`..`5`, `` ` ``, `6`..`9`, `0`, `P` and `X` — the same keys v1
uses, purple included in having none, because five keys are five labels only
if clearing is not one of them.

Three decisions in it were expensive to learn and cost nothing to keep.

**A rescan must never forget the stars.** The catalogue's update statement
lists only the columns the disk owns — length, write time, date taken,
dimensions, orientation. The rating, the label, the verdict and the words are
deliberately absent from it: those come from a person and are not ours to
overwrite because a file was touched. `a_rescan_does_not_forget_the_stars`
is the test, and it is the most valuable one in the file.

**A row before a file is opened.** Opening a folder writes an identity row for
every photograph in one batch and reads not a single file, so a rating has
somewhere to go from the first frame. Reading the headers is a separate pass
on a thread, and an `indexed` column is how it knows what is left; without it
a row that merely exists looks finished, and no date would ever be read.

| a folder opening | |
|---|---|
| 2,861 photographs | 16 ms |
| 134,990 photographs, recursive | **727 ms**, browsable and ratable at once |
| reading their headers, cold disk | 156 files/s, on a thread, while the grid works |
| the same headers, warm cache | 20,000 files/s |

Blocking on that read would have meant eighteen seconds of frozen window for
one folder of three thousand the first time it is opened — and Windows calls
a window that quiet unresponsive. The warm figure is what makes it look
harmless in a benchmark and is exactly the reason to measure the cold one.

**The selection follows the photographs, not the positions.** Reordering the
gallery rebuilds the selection by path. Otherwise the next rating lands on
whatever tile slid into that slot, which is a mistake nobody notices until
much later. For the same reason a rating deliberately does **not** reorder the
folder, even when it is sorted by rating: the tile would move out from under
the hand that just rated it, and culling is done by holding the keys down.

Two smaller ones. Keywords are their own table rather than v1's delimited
column, so finding every photograph with a word is an index lookup instead of
a `LIKE` over every row, and `COLLATE NOCASE` means nobody ends up with both
*Holiday* and *holiday* in the list. And the stars on a tile are **drawn, not
written** — the default font has no `★`, the same reason the folder tree draws
its own triangles.

Nothing is written to the photographs themselves yet. That is the metadata
outbox, and it comes next.

## Filtering and searching

The second block across from v1: **rating, colour label, verdict, format,
camera, lens, shape and capture date**, plus one search box over file names,
titles, descriptions and keywords.

Two rules make it usable rather than a puzzle, and both are v1's:

**An empty facet does not filter.** No labels chosen means *any* label, not
*no* label. The facets narrow each other and the values inside one widen it:
red and green means red **or** green; red plus three stars means red **and**
three stars.

**Only offer what is there.** The panel is built from the folder in front of
you — in a folder shot on two phones it offers those two phones and no
heading at all for lenses, because there are none. A list of every camera
ever owned, most of them matching nothing here, is a list nobody reads.

The filter is deliberately **not** saved between runs. One that survived a
restart would hide photographs on a later day for a reason nobody remembers
setting, and *where did half my folder go* is not a question an application
should ever cause. What is set is written on the button itself, in the accent
colour, with `3 of 2861` beside it — an active filter is never invisible.

Two things this does differently from v1. The search matches every **word**
typed rather than the line as one piece, so "iceland waterfall" finds a
photograph titled *Waterfall* with the keyword *Iceland*; one word behaves
identically. And a photograph with no date falls **outside** every date range
rather than inside all of them — asking for last July and being handed every
undated file is not an answer.

Reading the camera meant a little more EXIF, and one rule worth keeping:
`NIKON CORPORATION` and `NIKON Z 6` are one camera. Joined as they come they
stutter, so only the maker's first word is used and it is dropped when the
model already begins with it. `LensModel` is an EXIF 2.3 tag that phones and
older bodies simply do not write — where a Nikon keeps it is in its own
MakerNote, which is precisely the vendor breadth this deliberately does not
chase.

The gallery is now the folder plus a list of which rows get through it, so
typing in the search box costs one pass over memory rather than one query.
Over a hundred thousand photographs that is the difference between a search
box and a stutter.

## Writing into the photographs

The stars, the label and the words go into the files themselves, so Lightroom,
digiKam, darktable and Explorer all see the same thing — **without exiftool**.
What PhotoSite writes is five XMP properties and two EXIF numbers; exiftool's
worth is its breadth, and its cost is a 35 MB binary with its own copy of Perl,
once per platform.

It reads them too, which turned out to matter more than expected: a library
that has been used before arrives with metadata already in the files. On one
real folder of 2,861 photographs, **700 came with a rating and 267 with a
title** — put there by v1. A first scan takes them, and never overwrites
anything already said here.

Three rules, and every one of them is something that goes wrong quietly.

**Read before writing.** Handing a metadata writer a fresh, empty set means
"this is the whole of the file's metadata". A spike did exactly that to a real
photograph and turned 5,484 bytes of EXIF into 48 — capture date, camera,
exposure and orientation gone in one call. That spike is the reason this rule
is written down rather than assumed.

**Merge, never replace.** A packet holds other people's properties too. Ours
are taken out of wherever they were — child elements or attributes, under any
prefix — and written back as one block; everything else is copied through.
Properties are matched by namespace URI and never by prefix, because the same
namespace is `xmp:` to most tools and `xap:` to older ones, v1 included.

**Prove the photograph is untouched.** The compressed image is compared before
and after, in memory, and the write is refused if a single byte differs. On a
6.4 MB photograph the file grows by a few hundred bytes, settles on the second
write, and the picture is byte-identical.

Only JPEG is written into. Everything else — RAW above all — gets a `.xmp`
beside it: a sidecar cannot damage a photograph, and a format we cannot walk
segment by segment is one we cannot promise to preserve.

### The outbox

Writing a six megabyte file cannot happen on the way to the next frame, and
somebody culling a folder makes one of these on every keypress. So a change
goes into the catalogue and a queue in the same breath, and a thread drains it.

**The queue holds no payload.** v1 queued the change; this queues the
*photograph*, and what gets written is whatever the catalogue says at the
moment of writing. A retry therefore writes the truth as it stands rather than
a change that may since have been undone, rating a photograph twice leaves one
entry rather than two, and there is no way for the queue and the catalogue to
disagree. One row per photograph, so the primary key does the collapsing.

A file that cannot be written — open elsewhere, on a drive that is not there —
backs off and is tried again, eight times, and then stays in the queue with its
error rather than being thrown away. A fresh change clears the attempts,
because whatever stopped the last write may well be over.

The verdict is deliberately **not** written anywhere. There is no standard XMP
property for pick and reject; Lightroom keeps it in its own catalogue and so do
we, rather than inventing a private dialect nothing else reads.

## Getting about, and doing things to files

Back, forward and up with a clickable trail, on their own row rather than
crowded onto the toolbar — the path is the one thing there of no fixed width,
and a folder twelve deep would push everything else off the edge. Going back
and then somewhere new throws the forward trail away, the way a browser does:
a forward that leads somewhere nobody was heading is worse than none.

Rename, duplicate, a new folder, show in the file manager, and delete — which
means **the recycle bin, never `remove_file`**. The one place in this
application that could destroy a photograph for good is the one place that
should not exist.

Two rules hold across all of it.

**The catalogue follows the file.** A rename moves the row rather than
forgetting one and writing another, because the rating, the label and the
words all hang off its number. People rename files constantly and would never
think to be careful about it, so
`a_renamed_photograph_keeps_everything_said_about_it` is a test in the
catalogue and again over the whole application. A moved folder is done in the
catalogue rather than by rescanning both ends: five thousand photographs need
not give up their headers again to have been moved.

**The sidecar is part of the photograph.** A `.xmp` left behind under the old
name is everything that file held, lost — so it is renamed and deleted
alongside.

The folder is watched, so what another program does to it shows here. Bursts
are waited out rather than answered one at a time, and our own writing is
ignored: a rescan for every star anybody presses would be a rescan a second.

## Where this stands

The scaffolding is done and the photographic features are being brought over
on top of it. Browsing, culling, organising, filtering, searching, writing
metadata back into the files, getting about and the everyday file operations
all work; editing, batch conversion, import, faces and the AI describer are
still v1's alone.

| | |
|---|---|
| tests | 276 (including 6,000 fuzz cases over EXIF and 70 checked colour pairs) |
| scan of 7,558 photographs | 0.3 s; 0.1 s on a repeat |
| opening 134,990 photographs recursively | 727 ms |
| `cargo clippy -D warnings` | clean |
| themes | 5 plus following the system |
| settings entries | 29, screen generated from the descriptions |
| catalogue schema | 4 migrations |

A cross-check from Windows passes for `photosite-image` against both targets.
The rest does not, because `libsqlite3-sys` with `bundled` compiles C and that
needs a foreign toolchain — **the real verification for Linux and macOS is
done by CI**, where the runners are native.

## What is missing, and known to be

Of v1's features, in the order they are being brought across: RAW, comparison,
import, the editor, batch conversion, faces, the AI describer. Of the file
operations, copying and moving to a chosen folder are not done — the rest are.

Three of the filter's facets are waiting on features that come later: people,
the smiling and eyes-open scores, and the approximate-GPS verdict. All three
need something to filter on first.

RAW is deliberately small: the embedded JPEG every camera puts in the file, so
a folder off a card is never a grid of grey tiles. No demosaicing, no colour
science, no `rawler` — showing is all that is wanted of it.

Writing metadata will be pure Rust — `little_exif` for EXIF and `xmp-writer`
for XMP, rather than shipping exiftool and a copy of Perl. What PhotoSite
writes is some twenty tags in three namespaces; exiftool's worth is its
breadth, which is not what is needed here. Two things it did quietly will have
to be done on purpose: Windows Explorer reads the stars from EXIF `Rating`
while Lightroom reads `xmp:Rating`, so both get written; and the offline
reverse geocoding the AI describer used came out of exiftool's database and
needs a GeoNames extract in its place.

Beyond the features: a single instance, accessibility, signing and
notarisation for macOS, automatic updates. Of the languages, only English so far — cs-CZ is first in line.
None of it requires rewriting what is done.
