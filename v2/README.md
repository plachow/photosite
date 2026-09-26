# PhotoSite v2

A clean sheet. Rust, egui over wgpu, Windows / macOS / Linux from one source.

```bash
cd v2
cargo run --release -p photosite-ui              # the application
cargo run --release -p photosite-cli -- doctor   # where everything lives
cargo test --workspace                           # 553 tests, no window, no GPU
./packaging/pack.ps1                             # a Windows installer
```

The headless binary runs the whole of it — `scan`, `faces`, `name`,
`describe`, `convert`, `write`, `people`, `list` and `info` — which is how
the slow parts are measured on a real library, and how a runner with no
screen tests them.

## Layout

```
crates/
  photosite-core     domain, catalogue + migrations, paths, settings, log, tasks, commands
  photosite-image    decoding, downscaling, encoding, EXIF
  photosite-meta     what a photograph says about itself: XMP and EXIF, read and written
  photosite-faces    finding faces and telling them apart, on this machine
  photosite-batch    running a planned conversion
  photosite-ai       asking a vision model, here or with a key elsewhere, what is in a photograph
  photosite-ui       egui — the only crate that knows about the GPU
  photosite-cli      headless: runs the whole pipeline without a window
```

**None of the crates above the last two may carry `egui`, `eframe`, `wgpu` or
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

Two of them, and they answer different questions.

### On disk, so a folder opened before opens at once

Decoding a folder's tiles is a tenth of a second per photograph for the
eight-megabyte files a full-frame camera writes. Doing it again on every
start, for a library nobody has changed, is work that was already done — so a
finished tile is written beside the catalogue as a small JPEG and looked for
before anything is decoded.

| three hundred photographs, one thread | |
|---|---|
| the first time | 28.6 s (10 a second) |
| the second | **0.23 s** (1,316 a second) |
| kept | 6.9 MB, twenty-three kilobytes a tile |

**A cached tile is never stale, and there is no invalidation rule to get
wrong.** The name carries the file's length and write time along with its
path, so a photograph that changed asks for a name that has never been
written and is decoded. The old entry is not deleted; it simply stops being
asked for.

Every failure is a miss and never an error — an unwritable folder, a
half-written file, a disk that filled up all end in the photograph being
decoded, which is what would have happened anyway. Only tiles are kept: a
preview is a megabyte and is wanted one photograph at a time. A whole library
browsed through would be a few gigabytes, which is why
`loading.cache_thumbnails` can turn it off.

### In memory, so nothing is decoded twice in one sitting


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

## RAW

**Showing, and nothing more.** No demosaicing, no white balance, no colour
science, no `rawler`. Every camera puts a finished JPEG inside the RAW file it
writes — the rendering shown on the back of the camera — and that is what gets
drawn. It is a better answer than a half-built pipeline of ours would give,
and it costs a file read rather than a decoder per manufacturer.

What that buys is that a folder straight off a card is never a grid of grey
tiles. What it does not buy is editing a RAW, which is a different feature
and deliberately absent.

Nearly every RAW format is TIFF underneath — the file *is* the TIFF block,
where a JPEG merely carries one in a segment — so the same reader serves both,
and a RAW gives up its date, its camera and its orientation like anything
else. Canon's CR3 and Fuji's RAF are not handled: their containers are
something else entirely, and a file we cannot open is better left out of the
folder than shown as a grey tile with no explanation.

Two things worth knowing, both learned from real files rather than from a
specification:

**The largest preview, not the first.** A RAW carries several, from a 160x120
thumbnail upwards, and the small ones are listed first as often as not.
Drawing a 160-pixel thumbnail into a 400-pixel tile is the difference between
a photograph and a smear. Every block is walked, including the sub-blocks —
IFD0 of a Nikon file describes only its thumbnail — and the biggest one that
actually begins `FFD8` wins.

**Not every maker uses the ordinary tags.** Nikon, Canon, Sony and DNG name
the preview with the usual pair; Panasonic uses a tag of its own and nowhere
else, and gives the frame's size under two more that nobody else uses. Without
that, an RW2 has no size at all and drops out of a sort by dimensions.

Nothing here looks at a file's name: whether a file is a RAW is a question its
first two bytes answer. The one list of what counts as a photograph is in the
core, where the vocabulary lives, so it cannot come apart from a second copy.

| on a real library | |
|---|---|
| Nikon NEF | camera, date and 3040x2014 read; preview drawn in 17 ms |
| Panasonic RW2 | camera, date and 5480x3656 read; drawn in 10 ms |
| Adobe DNG | the same as the NEF it came from |

## Saying things about many photographs at once

The stars, the label and the verdict always went on the whole selection. The
words now do too: a title, a description and keywords typed with forty tiles
chosen land on all forty, and the panel says how many in the accent colour,
because writing a title over forty photographs by accident is not a small
mistake.

**Keywords are the exception, and they add rather than replace.** Editing one
photograph's keywords is editing a list somebody can see, so deleting a word
out of the box deletes the word. Forty photographs have forty different lists
and the box shows none of them, so setting would throw away everything already
on thirty-nine — and nobody typing "holiday" into a box means that.

A position stays with one photograph. Forty of them were not all taken in the
same spot, and one box for the lot would say they were.

## Two views of one folder

`Ctrl+L` turns the wall of tiles into a list of rows: name, when it was taken,
the camera, the frame, the size, and the stars, the label and the position mark
in a column of their own. Both are virtualised the same way, so the number of
photographs does not matter to either.

v1 kept the list in a panel beside the grid. This is a **switch** instead: two
views of one thing competing for the same width means both are too narrow, and
nobody reads a table four columns wide.

Along the way: `Ctrl` and the wheel resize the tiles the way they do in every
file manager, `F` fills the screen, and a photograph named on the command line
opens the folder it is in, standing on that photograph.

## Where it was taken

A position in a file looks like a fact and often is not. A phone that cannot
see the sky asks the cell towers instead and writes the answer down with the
same six decimal places it would use for a satellite fix; a camera holds the
last fix it got and stamps it on a photograph taken twenty minutes later in
the next valley. Both come out as coordinates, and neither says so.

So the file's evidence is weighed rather than trusted, and the verdict is
shown: a **traffic-light dot** beside the coordinates, and a **pin on the
tile** for the ones worth a second look. It never refuses to place a
photograph — it says how sure it is and lets somebody go and see. The
**Position** rows in the filter then collect them, which is the only reason
to mark them at all.

Three things count against a position, and the worst one stands: the camera's
own error estimate, a fix that came from the towers or the network instead of
the sky, and a fix that was already old when the shutter fired.

The **Map** button goes to an address from the settings holding `{lat}` and
`{lon}`, so which map is not our decision — it differs by country, by habit
and by whether somebody has an account anywhere. OpenStreetMap needs none of
those and is what a new installation starts with. The numbers are written with
a full stop and never by the locale's rules: a comma in a URL's coordinates
takes you to the wrong continent.

Two things came from real files rather than from the specification, and both
would have made the badge worthless:

**The shutter's clock and the satellites' clock are not the same clock.**
`DateTimeOriginal` is a wall clock with no zone attached; a GPS stamp is UTC.
Comparing them directly read as a one-hour-stale fix on **every** photograph
taken in this country. So the age of a fix is judged only when the file also
recorded what the camera's clock was set to, and otherwise not at all. A badge
that fires on everything says nothing.

**The method is written two different ways.** The specification says
`UNDEFINED` with a seven-byte character-set header; phones write a plain ASCII
string. Insisting on the specification read nothing from four hundred real
photographs, nine of which were fixed off a cell tower and were being called
precise for it.

| on 4,195 photographs from one phone | |
|---|---|
| precise | 2,684 |
| probably far off, fixed by cell tower | 1,460 |
| no position at all | 51 |

**Correcting one.** The coordinates are a box, not a caption: a position read
off a phone is a guess often enough that correcting one has to be as easy as
reading one. What is typed goes into the catalogue *and* into the photograph —
into the EXIF block and the XMP packet both — because the catalogue's copy is
overwritten by the next rescan and the file is the only place a correction can
live.

Writing one also **rewrites the story of how it was found**: the method
becomes `MANUAL`, and the camera's error estimate and the time of its fix are
removed, because they describe a fix that has just been replaced. A real
photograph taught us that: without it the mark came straight back the next
time the file was read, and correcting a position visibly did nothing.

A position is never *removed* from a file by us. A photograph whose
coordinates we happen not to hold is not one whose coordinates are wrong.

## Copying and moving

`Ctrl+C` and `Ctrl+V` mean the files themselves, not a list of their names —
they are the file manager's own keys and it would be strange for them to mean
anything else here. `Ctrl+X` cuts, `Alt+C` and `Alt+X` copy or move into a
folder chosen there and then, and `Ctrl+Shift+C` goes to wherever the last one
went, because sorting a folder into three piles is otherwise three dialogs and
two of them say the same thing.

**Nothing at a destination is ever written over.** A photograph landing on a
name already in use takes a number instead — `holiday (2).jpg` — and where
every file is going is worked out before a single byte moves. Two photographs
of the same name chosen from different folders do not land on each other
either: the plan claims each name as it goes. This is the one thing here that
could quietly destroy somebody's work, so it is decided in the core, where a
test can watch, and none of the deciding touches a disk.

A name whose **sidecar** belongs to somebody else is not free either. Dropping
`holiday.nef` into a folder that already holds a `holiday.xmp` would mean
reading a stranger's stars as this photograph's own, so that name is passed
over the same as a taken one. The sidecar travels with the photograph in every
operation — copy, move, rename, duplicate, delete — because it is part of it as
far as anybody is concerned, and for a RAW it is the only place its stars live.

A move across drives is not a rename. `std::fs::rename` refuses to cross a
volume and moving photographs between disks is exactly that, so a refused
rename becomes a copy and then a delete — in that order, since the other way
round loses the file when the copy fails. The catalogue follows a move the same
way it follows a rename: the row travels, and the stars with it.

### Two clipboards

**Ours** is a list of paths held in the application. It works on every
platform and it is what `Ctrl+V` reads when the system has nothing to say.

**The system's** is what makes a copy here paste in the file manager, and a
copy there paste here. Windows has one agreed way to put files on a clipboard
— `CF_HDROP`, with `Preferred DropEffect` alongside to say whether it was a
copy or a cut — and every other platform has several disagreeing ones. So the
system clipboard is spoken to on Windows and left alone elsewhere, where the
paths go on as text instead: pasting them into a terminal is a real use, and
claiming more than that would be pretending.

The system's answer wins when it has one. Somebody who copied a file in
Explorer and pressed `Ctrl+V` here means that file, not what they copied in
PhotoSite ten minutes ago. And a cut is spent once pasted, or the next `Ctrl+V`
would try to move the same photographs out of a folder they have left.

Under test the system clipboard is left alone entirely: a test that wrote to it
would take it out of the hands of whoever is running the tests, and one that
read from it would pass or fail by what they last copied.

| verified against Windows itself | |
|---|---|
| copy here | `FileDrop`, `FileNameW`, `FileName` and drop effect 1 |
| cut here | the same, drop effect 2, so Explorer moves rather than copies |
| copy in Explorer, paste here | the file arrives in the open folder |

## Comparison

`Ctrl+K` puts two to four photographs side by side, and the same key closes
them again. It is what a folder full of near-identical frames is for: three
of the same moment, one of them sharp, and no way to tell which without
looking at them together.

**One view, shared by all of them.** The pan and the zoom are a single thing,
so the wheel over any cell moves every cell and the same part of the scene is
in front of you in each. They are held as a fraction of the frame rather than
in pixels, which is what makes that true when the photographs are not the
same size — a phone shot beside a RAW shows the same part of the scene, not
the same number of pixels from the top left.

**The arrangement is worked out, not looked up.** Four portraits in a wide
window want a single row; four landscapes want two by two; three of anything
are usually better in a two by two with one cell empty than squeezed into a
row. Every arrangement is tried and the one that draws the photographs
largest wins, which is the only thing anybody opened a comparison for.

**A hundred per cent means a hundred per cent.** The percentage in the corner
is measured against the photograph as the file holds it — not against
whatever has been decoded so far, which would make the number mean nothing.
The comparison asks for its own decode at `loading.compare_size`, larger than
the preview: it is the one place somebody looks at pixel level to decide, and
a preview blown up past its own pixels answers that question wrongly.

The keys are the gallery's keys — the stars, the labels, pick and reject —
and they land on the photograph with the focus, not on the selection the
comparison opened with. `Tab` moves the focus round the ring; `Delete` takes
one out of the comparison and leaves the file exactly where it was; `Esc`
closes. `Delete` meaning two different things is deliberate and it is the
same thing said twice: get this out of what I am looking at. It is only in
the gallery that what one is looking at is the folder itself.

The arithmetic is all in the core and none of it needs a window: where each
photograph goes, which part of it is seen, how far in it will go and how the
space is divided. What is left in the drawing layer is a wheel notch turning
into a view, and a view turning into rectangles.

## One photograph on its own

A double-click on a tile — or `Enter` on the one the cursor is on — opens
the photograph in a tab of its own. So does handing the application a
photograph rather than a folder, which is what "open with" from a file
manager does: the folder opens behind it, standing on that tile. The strip
along the top holds the manager first, which cannot be closed, and after it
one tab per photograph.
A photograph opened twice has one tab: two tabs of one file would be two
views of one truth, and once the editor can change a photograph, two places
to change it. For now the tab is a viewer; the editor grows into it.

| in the editor | |
|---|---|
| the wheel, `PageDown`, `PageUp` | the next or the previous photograph, in the order the manager shows the folder — the same tab, retargeted, not a tab per page |
| `Ctrl` and the wheel, a drag | closer, and moved about; the same sums as the comparison |
| a middle click, `Ctrl+F` | fills the screen and gives it back; the strip of tabs goes with it |
| `Enter` | back to the manager, standing on this photograph: the folder opened if it has to be, the tree unfolded to it, the tile chosen and scrolled into view |
| `Esc`, the cross on the tab | closes the tab and leaves the manager where it was |

**The keys are commands, not keys.** `Escape`, `Enter` and the rest are
entries in the registry like every other, so they can be rebound with
everything else — and a command carries a **scope**: the manager, the
editor, or everywhere. The same key means one thing in one place and
another in the other (`Ctrl+F` is the filter over the grid and the whole
screen in the editor; `Enter` opens the editor from the manager and leaves
it from the editor), and that is not a conflict, because nobody is ever in
both. A conflict is a key two commands *reachable from the same place*
share, and the test says so.

**Closing asks nothing yet.** The tab holds nothing that is not in the file.
When it does, [`Editor::can_close`](crates/photosite-ui/src/editor.rs) is
the one place that has to learn to say no — the cross, the key and paging
away all ask it, so they will all ask the same way.

## Who is in the photograph

Four small networks, all of them files in a folder, none of them ours:
YuNet finds the faces, SFace turns each one into a hundred and twenty-eight
numbers, and two more read a smile and a blink. **Nothing leaves the
machine, and nothing needs to** — which is the whole reason for choosing
four models one can download over an API that would do all four jobs
better. A photograph of somebody's children is not something to upload in
exchange for a convenience.

They run through [tract](https://github.com/sonos/tract) rather than ONNX
Runtime, and that is the same trade this application makes about exiftool:
`ort` is faster and is a native library to ship once per platform or fetch
at run time, where tract is pure Rust and cross-compiles with everything
else. The cost is real — 75 ms to sweep a thousand-pixel frame and about
45 ms for every face found — and a sweep is a background pass over every
core, so it lands on the right side.

| a folder of holiday photographs | |
|---|---|
| 142 photographs, one pass | **2.8 s**, 113 faces, every one scored |
| the same folder again | nothing to do; a file is skipped by length and write time |

**A face hangs off the photograph's number, not off its path.** v1 keyed
its face rows by path, so a rename orphaned every face on a photograph and
the next sweep found them all again as strangers. Here the row travels with
the file exactly as its stars do, and a deleted photograph takes its faces
with it because the foreign key says so rather than because somebody
remembered.

### Three piles, in the order somebody works through them

**Suggestions** first: a face that is probably somebody already named. One
question, two answers, and nothing is written until the yes. **Groups**
next: faces nobody has named, gathered by likeness, named a whole group at
a time — which is the only thing that makes naming a library bearable.
**People** last, with every face said to be them, so a wrong match can be
taken back.

The three thresholds are the whole of the difficulty, and they are three
because three different things are being decided. Grouping strangers
together costs nothing if it is wrong — somebody looks at the pile and sees
a stranger in it — so it splits before it merges. Writing a name into a
file with nobody watching demands more. And the band between the model's
own boundary for "the same person" (0.363) and that certainty (0.5) is
exactly what a suggestion is for.

### What a name reaches

Naming a group writes the person's name into each photograph's **keywords**
and their face rectangle into the file as an **MWG region** — the format
Lightroom, digiKam and Windows read a face frame from. Both go through the
ordinary outbox, so a name arrives in a file exactly as a rating does, and
**without exiftool**: two details in the XMP decide whether anybody else
can read the result, and both are the kind that look right and are not. An
area is measured from its *centre*, not its corner; and a region list
without the frame's pixel dimensions is ignored outright.

One rule has a test of its own. A photograph nobody has face-scanned here
keeps whatever frames another program put in it: `None` is "we have not
looked", an empty list is "we looked and nobody here is named". The second
is what takes a name back out of a file. The first must never.

The name in the keywords matters more than it looks: it is what makes a
search for somebody find their photographs in every other program too, and
it is the only part of this that survives being read by software that has
never heard of face regions. It comes off a photograph only when the person
really has gone from it — two faces of one person on a group shot is
ordinary, and removing one wrong match must not take their name off a
photograph they are plainly still in.

### The smile and the blink

The same sweep scores every face for two things, with two more optional
models. **Both are optional on their own**: a face scanned before they were
on disk keeps its identity and its name, and a later pass scores it in
place by rectangle overlap — never losing its number or the person it
belongs to, which is exactly why it is a pass of its own rather than a
rescan.

Two counts and never one ratio, because "nobody has been scored yet" and
"everybody was scored and everybody is smiling" are different states and a
ratio cannot tell them apart. A face nobody has looked at is not evidence
of a frown. What comes of it: a quiet mark on a tile where something is
worth a second look, `1/2 smiling · 2/2 eyes open` in the details, and two
filter rows that cut a portrait series **both ways** — keep the frames
where everybody smiled, or collect the ones where somebody blinked. A
portrait cull is done from either end and both ends have to be one click
away.

The scores stay in the catalogue and are never written into a file. There
is no standard property for them and inventing one would be a private
dialect nothing else reads — the same reasoning as the pick and reject
verdict.

### Where it shows

A row of coloured dots on the tile, one per person, in **that person's own
colour** — kept out of the palette on purpose, so somebody wears the same
colour whatever theme is on and two badges of one colour mean the same
person without a name drawn three pixels high. The named faces framed over
the preview in the same colours. A People row in the details. And chips in
the filter, which are the one facet that composes by **conjunction**: every
other facet widens as values are added because they are alternatives, but a
photograph has any number of people and picking two means the shot they are
both in.

The models are not in git and not shipped — a hundred and fifty megabytes
of somebody else's binaries — and go in `models` beside the catalogue,
which `--data` moves like everything else. Without them the People window
says which four are missing and where it looked, rather than that "the
models are missing".

## Converting a lot of them at once

`Ctrl+B` over the selection — or over the whole folder when nothing is
selected. Format, size, sharpening, naming, numbering, where it goes and
what of the original travels with it, saved as named presets in the
catalogue.

**Every destination is worked out before a single byte is written**, and
that is the design rather than an optimisation: a batch is the one thing
here that can quietly destroy somebody's work, and a plan is what makes the
destruction visible while there is still time to change one's mind. So the
window says *forty to write · three left alone · one written over* and
shows the real path the first one would take, recomputed whenever a setting
changes. None of the deciding touches a disk, which is what lets a test
watch all of it.

Two collisions matter and they are different. A name already **on disk** is
one; a name another photograph **in this same plan** is about to take is
the other. v1 learned the second the hard way — two photographs called
`DSC_0042` from different folders, converted into one place, and the second
silently replaced the first — so the plan claims each name as it goes. And
a batch never writes over the photograph it is reading, whatever the
overwrite setting says: "write over it" means the files that were already
there, not the one being read.

### What travels, and what deliberately does not

Three tags are **not** carried onto a conversion, and every one of them
would be a bug that looks like a broken photograph:

* **The orientation.** The output was rendered the right way up, because
  the decode turned it — so an orientation tag from the source would turn
  it again. A folder of exported photographs all lying on their side is
  exactly what that looks like.
* **The pixel dimensions.** The output may have been resized, and a tag
  saying 6000x4000 over a 2048-pixel file is a lie that some readers
  believe.
* **The position**, when asked for. That is the whole reason somebody
  exports at all before putting a photograph of their house on the
  internet.

Everything else does travel: when it was taken, with what, at what
exposure, and the rating, the label, the words and the keywords. No
sidecars are written beside a conversion — a folder of exported files with
a `.xmp` next to each one is not what anybody meant by "export".

| twenty-five photographs, "For sharing" | |
|---|---|
| longest side 2048, quality 85, sharpened | 4.0 MB → 0.9 MB apiece |
| turned upright, position left behind, everything else carried | |

**WebP is written lossless, and that is worth saying out loud.** The only
pure-Rust WebP encoder there is encodes losslessly; a lossy one means
shipping libwebp, a C library to build once per platform. A lossless WebP
of a photograph is *larger* than the JPEG it would replace, so the dialog
says so where the quality slider would have been, and the starter preset
for the web is a JPEG. A control that does nothing is worse than no
control.

One photograph failing never ends a run. A file open in something else, a
JPEG that turns out to be a text file — each costs its own output, is
counted, and is **named** in the summary, because "three failed" is a
sentence nobody can act on.

## Asking what is in the photograph

`Ctrl+Shift+A` sends each photograph to a vision model on a
[local Ollama](https://ollama.com) server, which answers with a title, a
description and keywords. They go into the catalogue and the outbox exactly
as a typed title does, so they end up in the files too. Keywords are
**added** and never replace what a person wrote; by default only empty
titles and descriptions are filled, which is what makes an interrupted
overnight run restartable — everything already described is skipped.

The local model is the default and the feature was built round it. With a
key somebody brought, the same window asks **OpenAI** (or anything that
speaks its dialect — OpenRouter, Groq, Mistral, a local LM Studio),
**Anthropic** or **Google Gemini** instead: the same prompt, the same
schema, the same place context, and the answer read out of each
provider's own envelope. The choice is a *provider* said in words, not an
address that happens not to be localhost, because it is a change of
promise — the photographs leave the machine. The key lives in the settings
file on this machine, or in `PHOTOSITE_AI_KEY` for the headless binary,
and appears in no log and no error; a provider's complaint is quoted, the
request that provoked it is not. A busy provider (429, 5xx) is asked
again with a growing pause before a photograph is given up on.

The reply is held to a **JSON schema**, so the answer is always
machine-readable rather than prose that has to be picked apart with a
regular expression and an apology. Everything that can be decided without a
server — the prompt, the schema, reading the two layers of JSON, tidying
what comes back — is a module with tests of its own, because that is where
the quality of the descriptions actually lives.

The window shows the pace and a finishing time. That is not decoration: a
model takes tens of seconds a photograph, so a folder is minutes to hours,
and the only question anybody has while it runs is whether to wait.

### The place is told to the model, never asked of it

A local model shown a coastline will name a country with total confidence
and be wrong. So a photograph with coordinates is reverse-geocoded first
and the model is handed the answer as a fact — and where the position is
itself doubtful, it is handed the doubt too, so the wording softens to
"probably" instead of pretending to a precision the fix never had.

A **nearby** place reads "in or near X"; a distant one is only a reference
point — "about 39 km east of X" — because claiming a photograph was taken
in a town two valleys away puts a wrong name in a caption and the model
repeats it word for word. The resolved names also lead the keyword list,
most specific first, so a photograph is findable by region and country
however the model chose to phrase things; the place itself drops out beyond
ten kilometres or on a doubtful fix.

This is the last thing exiftool was still doing for this application. In
its place is a [GeoNames](https://download.geonames.org/export/dump/)
extract — a tab-separated file anybody can download and read — held in
memory and searched in a one-degree grid, so a lookup reads nine buckets
and a few hundred places rather than a hundred and fifty thousand. Put
`cities5000.txt` (and, for the region and country names,
`admin1CodesASCII.txt`, `admin2Codes.txt` and `countryInfo.txt`) in
`places` beside the catalogue. Without them there is simply no place
context, and the window says so plainly rather than leaving somebody to
work it out from a hundred vague descriptions.

**With Ollama, nothing leaves the machine**, and the gazetteer is why that
is still true of the place names: a coordinate sent to a geocoding service
is a photograph's location handed to somebody. For a long while the HTTP
client carried no TLS at all — the address was localhost, and shipping a
TLS stack to reach a local socket was a trade this application did not
make. The cloud providers changed that: `photosite-ai` carries rustls now,
and so, through it, does the headless binary.

Where the chosen language is not English, the same call also returns an
**English description**, kept in the catalogue and never written into the
file. The photograph carries one description, in the language somebody
asked for; the second copy is what makes a library described in Czech
findable by somebody typing English.

## Where this stands

**Everything v1 did outside the editor, v2 does.** Browsing, culling,
organising, filtering, searching, writing metadata back into the files,
positions and the map, getting about, the file operations, showing RAW,
comparing, the list view — and now finding and naming faces, reading a
smile and a blink, converting a folder at once, and asking a model on this
machine what is in a photograph. The editor is what is left, and it is
deliberately last.

| | |
|---|---|
| tests | 553 (including 6,000 fuzz cases over EXIF and 70 checked colour pairs) |
| scan of 7,558 photographs | 0.3 s; 0.1 s on a repeat |
| opening 134,990 photographs recursively | 727 ms |
| face sweep of 142 photographs | 2.8 s, 113 faces, every one scored |
| converting 25 to 2048 px | 4.0 MB → 0.9 MB apiece |
| describing with a 27B model | about 6 s a photograph |
| `cargo clippy -D warnings` | clean |
| themes | 5 plus following the system |
| settings entries | 47, of which 38 are on the screen it generates |
| catalogue schema | 8 migrations |
| commands in the registry | 59 |

A cross-check from Windows passes for `photosite-image` against both targets.
The rest does not, because `libsqlite3-sys` with `bundled` compiles C and that
needs a foreign toolchain — **the real verification for Linux and macOS is
done by CI**, where the runners are native.

### What has to be fetched

Two things are downloaded rather than shipped, because both are large and
neither is ours. Without either, the feature that needs it says so and
everything else carries on.

| | where | what for |
|---|---|---|
| four ONNX models | `models` beside the catalogue | finding faces, and reading a smile and a blink |
| a GeoNames extract | `places` beside the catalogue | naming the place in a description |

`--data` moves both, like everything else.

## Getting it onto a machine that has no toolchain

```powershell
cd v2
./packaging/pack.ps1
```

An installer — `artifacts/releases/PhotoSite-win-Setup.exe`, about 37 MB —
that installs without an administrator into `%LOCALAPPDATA%`, makes the two
shortcuts, and appears in *Apps & features* to be removed again. Beside it a
portable zip, for whoever would rather have a folder; `PHOTOSITE_DATA` means
that folder genuinely is portable.

The tool is [Velopack](https://velopack.io), the same one v1 shipped with,
because the installer and the update are one artefact and one feed rather
than two, and because it asks for no code signing certificate. What it *is*
asking for, and the several other tools that were weighed and rejected, is in
[packaging/README.md](packaging/README.md).

**On a machine that ran v1, move `%LOCALAPPDATA%\PhotoSite` aside first.**
The package is `PhotoSite` and installs there, and Velopack empties the
install folder before it writes — which is where v1 kept its catalogue and
its thumbnails. That is the whole of the collision: v2's own catalogue is in
`%APPDATA%`, is not touched by an install and survives an uninstall.

Two consequences worth knowing about, both in
[`startup.rs`](crates/photosite-ui/src/startup.rs). The binary is linked for
the windowed subsystem, so an installed application does not put a black
rectangle beside its own window; it borrows the terminal's console back when
it was started from one, which is what keeps `--selftest` able to say
anything. And `VelopackApp::run()` is the first statement in `main`, before
the settings are read and before there is a window, because the installer and
the updater start this executable to do their work and then end it.

## What is missing, and known to be

**The editor**, which is the last of v1 and will be rebuilt rather than
ported: crop and geometry, the adjustment sliders, the histogram, filters,
annotation layers, before-and-after, and export with its own preset list.
The preset store already has a second list waiting for it. What is there so
far is its tab and its keys — a photograph on its own, paged through the
folder — see *One photograph on its own* above.

Importing from a memory card is **not** being brought across at all — this
is a manager for files that are already on a disk. Nor is uploading to
Imgur, which belonged to the editor and to a service.

WebP is written **lossless**: the only pure-Rust encoder there is encodes
losslessly, and a lossy one means shipping libwebp. The dialog says so
rather than leaving somebody to find out from a folder of unexpectedly
large files.

Beyond the features: a single instance, accessibility, signing —
SmartScreen shows its blue panel over an unsigned installer until enough
people have clicked through it — and notarisation for macOS. **Updating
itself** is half done: what the updater invokes is in place, and what has no
code yet is the asking — an `UpdateManager` over the published releases, off
the main thread, and somewhere in the window to say a new version is there.
Deliberately not written before there is a release to update from.

Of the languages, only English so far — cs-CZ is first in line. None of it
requires rewriting what is done.
