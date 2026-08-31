# PhotoSite — en-US
#
# The source language and the fallback in one: whatever is missing from
# another translation is taken from here. Keys are never changed or renamed,
# only added — translations on other people's disks are built from them.

## Command groups

group-file = File
group-view = View
group-help = Help

## Commands

command-file-open-folder = Open folder…
command-file-rescan = Reload folder
command-file-quit = Quit
command-view-recursive = Include subfolders
command-view-toggle-tree = Folders
command-view-toggle-preview = Preview
command-view-toggle-info = Info
command-view-reset-layout = Reset the layout
command-view-bigger-tiles = Larger tiles
command-view-smaller-tiles = Smaller tiles
command-view-next-theme = Next theme
command-view-settings = Settings…
command-help-diagnostics = Diagnostics

## Docks
#
# The names of the panes the window layout is assembled from.

dock-tree = Folders
dock-gallery = Photographs
dock-preview = Preview
dock-info = Photo info
docks-all-hidden = Every dock is hidden

## Themes

theme-dark = Dark
theme-light = Light
theme-grey = Grey
theme-sepia = Sepia
theme-seabreeze = Sea Breeze
theme-automatic = Follow the system

## The grid

toolbar-recursive = Recursive
gallery-empty = No photographs here
gallery-pick-folder = Open a folder to begin
preview-pick-tile = Click a tile to see it here

# The plural is here on purpose: Czech will need three forms, and a format
# that cannot do that would later have to be rewritten along with its call
# sites.
gallery-count =
    { $count ->
        [one] { $count } photograph
       *[other] { $count } photographs
    } in { NUMBER($ms, maximumFractionDigits: 0) } ms

## Dialogs

# The title of the native dialog. Windows and macOS show it in the title bar,
# the XDG portal in the strip at the top — nowhere is it discarded, so it has
# to be translated.
dialog-pick-folder = Choose a folder with photographs

## Settings
#
# Field labels. The keys keep the shape setting-<path with dashes>, so one can
# be reached from the other without searching.

settings-title = Settings
settings-reset = Reset to defaults
settings-reset-section = Reset this section
setting-window-width = Window width
setting-window-height = Window height
setting-window-x = Window position, horizontal
setting-window-y = Window position, vertical
setting-window-maximized = Window maximised
setting-window-layout = Dock layout
setting-window-docks-hidden = Hidden docks
setting-window-splitter = Splitter thickness

setting-tile-size = Tile size
setting-gap = Gap between tiles
setting-tile-aspect = Tile image ratio
setting-caption-height = Caption strip height
setting-tile-padding = Tile padding
setting-prefetch-rows = Rows loaded ahead
setting-show-captions = Show file names
setting-recursive = Include subfolders
setting-last-folder = Last folder

setting-thumb-size = Thumbnail size
setting-preview-size = Preview size
setting-uploads-per-frame = Uploads per frame
setting-texture-budget = Images kept in memory
setting-worker-threads = Decoding threads
setting-use-embedded = Use embedded EXIF thumbnails
setting-idle-repaint = Idle repaint interval (safety net)

setting-theme = Theme
setting-theme-dark = Theme when the system is dark
setting-theme-light = Theme when the system is light
setting-language = Language
setting-ui-scale = Interface scale

## Photo details

info-pick-tile = Click a tile to see its details
info-name = name
info-folder = folder
info-size = size on disk
info-size-mb = { NUMBER($mb, maximumFractionDigits: 1) } MB
info-orientation = orientation
info-embedded = EXIF thumbnail
info-embedded-at = { $bytes } B
info-embedded-none = none
info-preview-px = { $width }×{ $height }
info-preview = preview decoded
info-preview-waiting = decoding…

## Diagnostics

diagnostics-title = Diagnostics
diagnostics-version = version
diagnostics-platform = platform
diagnostics-mode = mode
diagnostics-mode-portable = portable
diagnostics-mode-system = system
diagnostics-data = data
diagnostics-config = settings
diagnostics-cache = cache
diagnostics-logs = log
diagnostics-catalog = catalogue
diagnostics-schema = schema
diagnostics-workers = worker threads
diagnostics-photos = photographs in folder
diagnostics-textures = textures in memory
diagnostics-decoding = decoding now
diagnostics-tasks = running tasks
diagnostics-blank = blank tiles
diagnostics-unsharp = tiles not yet sharp
diagnostics-language = language

## Self-check

selftest-ok =
    self-check passed: { $count } photographs, no blank tile, { NUMBER($ms, maximumFractionDigits: 0) } ms
selftest-no-photos = SELF-CHECK FAILED: the folder holds no photographs
selftest-blank =
    SELF-CHECK FAILED: { $count } tiles still blank after { NUMBER($seconds, maximumFractionDigits: 0) } s

## Command line

cli-scan-found = found { $count } photographs in { NUMBER($ms, maximumFractionDigits: 0) } ms
cli-scan-done =
    wrote { $added }, unchanged { $skipped }, unreadable { $failed } in { NUMBER($seconds, maximumFractionDigits: 1) } s ({ NUMBER($rate, maximumFractionDigits: 0) } files/s)
cli-catalog-total = { $count } photographs in the catalogue
cli-list-total = { $count } photographs
cli-info-path = path
cli-info-size = size
cli-info-modified = modified
cli-info-orientation = orientation
cli-info-embedded = EXIF thumbnail
cli-info-embedded-at = { $bytes } B at offset { $offset }
cli-info-embedded-none = none
cli-info-quick = quick preview
cli-info-quick-none = cannot be produced
cli-info-tile = tile
cli-info-size-px = { $width }×{ $height } in { NUMBER($ms, maximumFractionDigits: 1) } ms

## Errors

error-not-a-folder = { $path } is not a folder
error-not-a-file = { $path } is not a file
