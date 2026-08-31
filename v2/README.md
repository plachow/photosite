# PhotoSite v2

A clean sheet. Rust, egui over wgpu, Windows / macOS / Linux from one source.

```bash
cd v2
cargo run --release -p photosite-ui              # the application
cargo run --release -p photosite-cli -- doctor   # where everything lives
cargo test --workspace                           # 114 tests, no window, no GPU
```

## Layout

```
crates/
  photosite-core     domain, catalogue + migrations, paths, settings, log, tasks, commands
  photosite-image    decoding, downscaling, EXIF
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

## Where this stands

The scaffolding is done, the photographic features are not. The grid, the
tree, the preview and three themes come from the prototype, so there is
something to run.

| | |
|---|---|
| tests | 114 (including 6,000 fuzz cases over EXIF and 70 checked colour pairs) |
| scan of 7,558 photographs | 0.3 s; 0.1 s on a repeat |
| opening a folder in the UI | 7,558 photographs, no blank tile after 160 ms |
| `cargo clippy -D warnings` | clean |
| themes | 5 plus following the system |
| settings entries | 27, screen generated from the descriptions |

A cross-check from Windows passes for `photosite-image` against both targets.
The rest does not, because `libsqlite3-sys` with `bundled` compiles C and that
needs a foreign toolchain — **the real verification for Linux and macOS is
done by CI**, where the runners are native.

## What is missing, and known to be

Watching the disk for changes (`notify`), a single instance, accessibility,
signing and notarisation for macOS, automatic updates. Of the languages, only
English so far — cs-CZ is first in line.
None of it requires rewriting what is done.
