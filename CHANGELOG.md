# Changelog

All notable changes to PhotoSite are recorded here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the versions
follow [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

Every release's section becomes the notes of that GitHub release, which is
the page the *What's new?* link on the status bar opens. A user-facing
change lands here under **Unreleased** in the same commit that makes it.

## [Unreleased]

### Added

- A slider on the toolbar sets the size of the tiles, in ten steps from 80 to
  640 pixels, and the size is kept for the next run.

### Changed

- Larger tiles and Smaller tiles step through the same sizes as the new
  slider instead of growing by a quarter from wherever the size was.

- The filter offers people in the order of how many photographs in the
  folder each is on, the most photographed first, rather than by name.
- The people in the filter take at most four rows; a folder with more
  people scrolls them there instead of growing the dialog.

### Fixed

- Rating, describing or naming people in a JPEG no longer wipes the
  camera's EXIF from it. A block the writer could not read — every photograph
  from a HONOR phone, which labels one tag differently from the standard —
  was taken for no EXIF at all and replaced, losing the date taken, the
  camera and the exposure. Such a block is now read correctly, and one that
  still cannot be read, or would lose anything in the rewrite, is left as
  it is; the stars and the words then go into XMP and IPTC only.
- The sensitivity (ISO) of photographs from cameras that store it as a long
  number is read instead of showing as none.
- Writing a rating, a label, words or names into a photograph keeps the
  file's creation date; only the modified date moves, as backups and
  synchronisation need it to.
- A photograph deleted outside PhotoSite, in Explorer or on a network share,
  leaves the gallery the next time the folder is read (F5, or when the
  folder's change is noticed) instead of staying as an empty tile. A folder
  that cannot be read in full forgets nothing.
- Copy to, Duplicate and a Move to another drive keep the original's dates:
  the copy is created and modified when the photograph was, not when it was
  copied.
- The filter opens as wide as it was last made, in the next run too.
- The filter is as tall as its sections, up to the height of the
  application, so the dates at the bottom no longer need scrolling to on a
  screen with room for them.

## [2.3.0] - 2026-11-15

### Added

- A photograph open in a tab of its own, full screen included, can be rated
  there: `1` to `5` and `` ` `` land on the photograph being looked at, and
  the row beneath it shows its stars, which can be clicked as well.
- The stars in the details pane are preceded by a mark for no rating, so
  taking the stars off no longer means knowing to click the last one again.
- A tile carries a small mark when its photograph has a title and a
  description, and the filter can show only the described photographs or
  only the ones still to be described.
- Resting the pointer on a mark on a tile says what the mark means.
- Hover texts name the key that does the same thing: the stars, the labels,
  the verdict and the toolbar.
- Clicking into a date box of the filter opens a calendar under it. A date
  can also be typed day first, as in `20.5.2025`, and one that cannot be
  read is shown in red rather than quietly ignored.
- With a photograph in a tab of its own, full screen included, the arrows
  turn the page: Down and Right to the next photograph, Up and Left to the
  previous. `Ctrl+PageDown` and `Ctrl+PageUp` go ten at a time.
- `Alt+1` to `Alt+9` put a person on the photographs selected, or on
  the one filling the screen, and take them off again: for somebody the
  face scan did not find, turned away or far off. The nine places are the
  names written in the settings under *People by key*, or the first nine
  people by name when none are. The row beneath a photograph in a tab of
  its own shows who is on it.

### Changed

- The dots for the people on a tile are raised a little, with a shadow and
  a glint, rather than flat discs.
- The stars are drawn with smooth edges, and the stars not given are
  outlines rather than grey stars.
- The marks along the bottom of a tile stand in one row on its bottom edge:
  the star, the dots for the people and the mark for a description. The
  dots used to sit a third of the way up the photograph.
- The icon is darker and its colours stronger, drawn in flat shapes with
  hard edges: a sunset in a dark frame on a pile of two prints. The pastel
  one was hard to pick out on a taskbar.

### Removed

- The mark on a tile for a blink or a missing smile. The counts in the
  details and the two rows of the filter are still there.

### Fixed

- The filter no longer jumps about under the pointer. A value not chosen
  grew a little when the pointer came onto it, pushing the ones after it
  along and the last of the row onto the next line.
- On a keyboard whose row of digits types something else without Shift, a
  Czech one for instance, every key of that row now rates and labels by
  where it sits. The first key, which types `+`, used to do nothing.
- The cross that takes a person off a photograph is a cross and no longer
  an empty box.

- The selection no longer vanishes every few seconds while something else
  is changing the open folder, such as OneDrive syncing the photographs a
  face sweep has just written names into. The folder is read again, but
  whatever was selected stays selected.
- A rating, a title, a description and keywords given in another program
  are now taken from a photograph whose phone wrote so much of its own
  ahead of them that they sat past the part of the file a folder is read
  from. They were in the file and the photograph showed as unrated.
  Every JPEG already in the catalogue is read once more, a folder at a
  time as each is opened, so the ones missed until now are found; nothing
  said here is overwritten.

## [2.2.0] - 2026-10-22

### Added

- The Describe window and the face sweep report their progress in one line:
  how far, the average time per photograph, how long is left and when they
  will finish, in local time.
- The People window can sweep only the selected photographs, and can be
  asked to look at every photograph again rather than only the new ones.
- Face thumbnails are larger, show a closer look under the pointer and open
  large in place on a click.
- When the model describes in a language other than English, the title and
  the keywords are kept in English in the catalogue as well as the
  description, so a search finds the photograph in either language. Only
  the chosen language is written into the photograph.
- The status bar offers a newer release when one is published, with a link
  to what changed and an *Update now* button. Nothing is downloaded until
  the button is pressed.
- The People window offers to download the face models when they are not on
  disk, saying what each file is, who publishes it and how large it is.

### Changed

- Every dialog opens in the middle of the window the first time and where
  it was left after that, also across restarts. A position that has fallen
  off the screen is dropped and the dialog comes back to the middle.
- The dialogs share one layout: the same margins, more room between rows, a
  form grid in the Describe window, and the main button in the accent colour
  on a footer of its own.
- The Describe window has one *Language* field, which says what is written
  into the photographs; the English copy for the catalogue is automatic.
- The note about a missing place list is readable, and the long folder path
  is shortened with the whole of it shown under the pointer.

### Fixed

- Typing a name for a group of faces or renaming a person and pressing
  Enter did nothing. It names now, and *OK* and *Cancel* stand beside the
  field; Escape cancels.
- The same fault in the preset name field of the Convert window.

### Removed

- The retired C# and WPF implementation (v1) is gone from the tree. Its
  last state remains under the tag `v1-final`.

## [2.1.0] - 2026-10-19

### Added

- A menu bar, driven from the keyboard the way the platform does it, and a
  status bar.
- Favourite folders above the folder tree, in bold, with a cross to drop
  one; Ctrl+Shift+B marks the open folder as one.
- A menu on each tile: copy, copy to, move to, turn, open with, and the
  system's own menu.
- Copy to and Move to open where the last one went and remember their place;
  a recent folder can be struck off the list.
- A picture from the clipboard opens as a new image.
- Ctrl+C, Ctrl+X and Ctrl+V work from the keyboard; Ctrl+L and Ctrl+R turn
  a photograph.
- The gallery can be walked and selected from the keyboard, Explorer's way.
- Editor keys: + and - for a step closer and back, * for one pixel per
  point, 0 for the whole photograph.
- Describing can ask OpenAI, Anthropic or Gemini with a key of your own,
  beside Ollama on this machine.
- The title, description and keywords are written into EXIF and IPTC as
  well as XMP, and read back from there.
- Who is on a photograph can be said by hand from the details pane.
- The settings window has sections down the left and a search above.
- The application has its own icon.

### Changed

- The manager opens first instead of an empty window.
- The folder tree looks like Explorer's: a plus only where there is
  something to unfold, a scrollbar that stays, Ctrl+wheel that zooms.
- One star with the number in it, coloured by how many.
- The wheel moves the gallery by more than a line, and the setting says how
  much.
- The interface scales over the display's own scale, not instead of it.
- The doubtful-position pin is a setting, off by default.
- Every label starts with a capital letter.

### Fixed

- The catalogue's write lock is taken as a transaction begins, not halfway
  through.
- A photograph renamed while its header was being read no longer gets a
  row of its own.
- The Windows-only parts of the shell stay quiet on macOS and Linux.

## [2.0.0] - 2026-09-24

The first release of the second PhotoSite: a clean sheet in Rust, egui over
wgpu, for Windows, macOS and Linux from one source. The earlier C# and WPF
implementation stopped at 0.9.x and never shipped.

### Added

- A photo manager: a folder tree, a thumbnail grid that stays smooth over
  tens of thousands of photographs, a list view, a quick preview, filters,
  search and a tally.
- Organising: stars, colour labels, flags, titles, descriptions and
  keywords, written into the photographs as XMP and read back from them.
- Where a photograph was taken, resolved against an offline place list, with
  a verdict on how far to trust a shaky GPS fix.
- Faces: found, told apart and named entirely on this machine, with smiles
  and open eyes scored and filterable, and the regions written where other
  programs read them.
- Descriptions from a vision model on this machine through Ollama, grounded
  in the real place rather than a guessed one, with an English copy kept
  for searching.
- RAW files, shown from the preview the camera put inside them.
- Side-by-side comparison of two photographs.
- Copy, cut, paste and move between folders, never over anything; the
  recycle bin rather than deletion.
- Batch conversion with a plan shown before anything is written.
- An editor with its own tabs, a menu, tools in windows of their own, and
  the finished image framed at its real size.
- A Windows installer that updates itself, and a `photosite-cli` for the
  same work without a window.

[Unreleased]: https://github.com/plachow/photosite/compare/v2.3.0...HEAD
[2.3.0]: https://github.com/plachow/photosite/compare/v2.2.0...v2.3.0
[2.2.0]: https://github.com/plachow/photosite/compare/v2.1.0...v2.2.0
[2.1.0]: https://github.com/plachow/photosite/compare/v2.0.0...v2.1.0
[2.0.0]: https://github.com/plachow/photosite/releases/tag/v2.0.0
