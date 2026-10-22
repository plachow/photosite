# Changelog

All notable changes to PhotoSite are recorded here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the versions
follow [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

Every release's section becomes the notes of that GitHub release, which is
the page the *What's new?* link on the status bar opens. A user-facing
change lands here under **Unreleased** in the same commit that makes it.

A version with a `-beta.N` suffix is a pre-release: published as one on
GitHub, offered only to those already running a beta, and not yet whole.
The plain version that follows is the release.

## [Unreleased]

## [2.2.0-beta.1] - 2026-10-22

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

[Unreleased]: https://github.com/plachow/photosite/compare/v2.2.0-beta.1...HEAD
[2.2.0-beta.1]: https://github.com/plachow/photosite/compare/v2.1.0...v2.2.0-beta.1
[2.1.0]: https://github.com/plachow/photosite/compare/v2.0.0...v2.1.0
[2.0.0]: https://github.com/plachow/photosite/releases/tag/v2.0.0
