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
setting-compare-size = Comparison size (px)
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

## Culling
#
# The stars, the labels and the verdict. The keys 1..5 and 6..9 do these;
# the names are here so a menu and a tooltip say the same thing.

group-photo = Photo
group-sort = Sort

command-photo-rate-0 = No rating
command-photo-rate-1 = One star
command-photo-rate-2 = Two stars
command-photo-rate-3 = Three stars
command-photo-rate-4 = Four stars
command-photo-rate-5 = Five stars

command-photo-label-none = No label
command-photo-label-red = Red label
command-photo-label-yellow = Yellow label
command-photo-label-green = Green label
command-photo-label-blue = Blue label
command-photo-label-purple = Purple label

command-photo-pick = Pick
command-photo-reject = Reject
command-photo-select-all = Select all

# The label on its own, for a swatch or a chip. Shorter than the command,
# which has to say what pressing it does.
label-none = None
label-red = Red
label-yellow = Yellow
label-green = Green
label-blue = Blue
label-purple = Purple

flag-none = Undecided
flag-picked = Picked
flag-rejected = Rejected

## Sorting

sort-taken = Date taken
sort-name = File name
sort-rating = Rating
sort-modified = Date modified
sort-size = File size
sort-dimensions = Dimensions

command-sort-reverse = Reverse the order
toolbar-sort = Sort
toolbar-sort-ascending = Oldest and smallest first
toolbar-sort-descending = Newest and largest first

setting-sort-field = Sort the gallery by
setting-sort-descending = Sort the other way round

## The selection
#
# Three plural forms are coming in Czech, so the count goes through the same
# selector as everything else counted.

gallery-selected =
    { $count ->
        [one] { $count } selected
       *[other] { $count } selected
    }

## What somebody said about a photograph

info-rating = rating
info-label = label
info-flag = verdict
info-title = title
info-description = description
info-keywords = keywords
info-keywords-hint = Separate them with commas
info-nothing-said = nothing yet
info-many-selected =
    { $count ->
        [one] { $count } photograph selected
       *[other] { $count } photographs selected
    }
info-many-hint = The stars, the label and the verdict go on all of them. The
    words are written one photograph at a time.

## Reading a folder

task-reading-folder = Reading the folder
task-reading-headers = headers

diagnostics-selected = selected

info-taken = taken
info-taken-none = not yet known
info-dimensions = dimensions
info-dimensions-px = { $width } × { $height }

cli-info-taken = taken at
cli-info-taken-none = no date in the file
cli-info-frame = frame
cli-info-frame-none = no size in the header

cli-info-camera = camera
cli-info-lens = lens
cli-info-none = not recorded

## The filter
#
# What is set has to be visible: half a folder missing with nothing on screen
# to say why is the worst thing a filter can do.

filter-none = Filter
filter-title = Filter
filter-clear = Show everything
filter-rating = { $count }★ and up
filter-cameras =
    { $count ->
        [one] { $count } camera
       *[other] { $count } cameras
    }
filter-lenses =
    { $count ->
        [one] { $count } lens
       *[other] { $count } lenses
    }
filter-date = date
filter-no-rejects = no rejects
filter-hide-rejected = Hide the rejects
filter-search = Search
filter-search-hint = File name, title, description, keywords
filter-showing =
    { $shown ->
        [one] { $shown } of { $all }
       *[other] { $shown } of { $all }
    }
filter-nothing-matches = Nothing here matches the filter

filter-section-rating = Rating
filter-section-label = Label
filter-section-flag = Verdict
filter-section-format = Format
filter-section-camera = Camera
filter-section-lens = Lens
filter-section-shape = Shape
filter-section-taken = Taken

shape-any = Any shape
shape-landscape = Landscape
shape-portrait = Portrait
shape-square = Square

diagnostics-showing = showing
command-view-filter = Filter…
command-view-clear-filter = Show everything

filter-any = Any
filter-from = from
filter-to = to
filter-taken-range = this folder spans { $from } to { $to }

task-writing-metadata = Writing into the photographs
diagnostics-unwritten = waiting to be written
diagnostics-unwritable = could not be written

cli-scan-seeded =
    { $count ->
        [one] took what { $count } photograph already said
       *[other] took what { $count } photographs already said
    }

## Getting about and doing things to files

group-go = Go

command-go-back = Back
command-go-forward = Forward
command-go-up = Up one folder

command-file-rename = Rename…
command-file-duplicate = Duplicate
command-file-delete = Move to the recycle bin
command-file-new-folder = New folder…
command-file-reveal = Show in the file manager

ask-rename = Rename
ask-new-folder = New folder
ask-name = Name
ask-confirm = OK
ask-cancel = Cancel

files-nothing-selected = Nothing is selected
files-renamed = Renamed
files-duplicated =
    { $count ->
        [one] { $count } copy made
       *[other] { $count } copies made
    }
files-deleted =
    { $count ->
        [one] { $count } photograph moved to the recycle bin
       *[other] { $count } photographs moved to the recycle bin
    }

command-photo-compare = Compare
command-photo-compare-next = Focus the next
command-photo-compare-previous = Focus the previous

compare-needs-two = Pick at least two photographs to compare
compare-only-four =
    { $count ->
        [one] Only the first is being compared
       *[other] Only the first { $count } are being compared
    }
compare-fit = Fit
compare-actual = 100 %
compare-magnification = { $percent } %
compare-hint = Tab focuses the next · Delete takes one out · Esc closes
compare-unreadable = Not decoded yet

command-file-copy = Copy
command-file-cut = Cut
command-file-paste = Paste
command-file-copy-to = Copy to…
command-file-move-to = Move to…
command-file-copy-again = Copy to the last folder

setting-last-destination = Last destination

files-copied =
    { $count ->
        [one] { $count } photograph copied
       *[other] { $count } photographs copied
    }
files-moved =
    { $count ->
        [one] { $count } photograph moved
       *[other] { $count } photographs moved
    }
files-on-the-clipboard =
    { $count ->
        [one] { $count } photograph on the clipboard
       *[other] { $count } photographs on the clipboard
    }
files-cut-to-the-clipboard =
    { $count ->
        [one] { $count } photograph ready to move
       *[other] { $count } photographs ready to move
    }
files-clipboard-empty = There are no photographs on the clipboard
files-nowhere-to-paste = Open a folder to paste into first
files-nowhere-yet = Nothing has been copied anywhere yet
files-already-there = They are already in that folder
copy-into = Copy into…
move-into = Move into…

place-nowhere = No position
place-precise = Precise
place-approximate = Approximate
place-doubtful = Probably far off
place-because-error = The camera reported a coarse position
place-because-method = The position came from cell towers or Wi-Fi, not satellites
place-because-stale = The fix was already old when the shutter fired
cli-info-place = Taken at
setting-map-url = Map address
info-place = Taken at
info-place-none = Not recorded
info-map = Map
info-place-hint = Coordinates, however you like to type them
filter-places = Position
info-exposure = Exposure
info-iso = ISO { $iso }
info-focal = { NUMBER($mm, maximumFractionDigits: 0) } mm
info-focal-equivalent = { NUMBER($mm, maximumFractionDigits: 0) } mm ({ $equivalent } eq)
info-keywords-hint-many = Separate them with commas. On a selection they are added, not replaced.
info-place-unreadable = Those are not coordinates
command-view-fullscreen = Fullscreen
command-view-as-list = List
setting-as-list = Show the folder as a list
