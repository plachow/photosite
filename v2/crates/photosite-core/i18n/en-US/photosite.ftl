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
info-name = Name
info-folder = Folder
info-size = Size on disk
info-size-mb = { NUMBER($mb, maximumFractionDigits: 1) } MB
info-orientation = Orientation
info-embedded = EXIF thumbnail
info-embedded-at = { $bytes } B
info-embedded-none = none
info-preview-px = { $width }×{ $height }
info-preview = Preview decoded
info-preview-waiting = decoding…

## Diagnostics

diagnostics-title = Diagnostics
diagnostics-version = Version
diagnostics-platform = Platform
diagnostics-mode = Mode
diagnostics-mode-portable = portable
diagnostics-mode-system = system
diagnostics-data = Data
diagnostics-config = Settings
diagnostics-cache = Cache
diagnostics-logs = Log
diagnostics-catalog = Catalogue
diagnostics-schema = Schema
diagnostics-workers = Worker threads
diagnostics-photos = Photographs in folder
diagnostics-textures = Textures in memory
diagnostics-decoding = Decoding now
diagnostics-tasks = Running tasks
diagnostics-blank = Blank tiles
diagnostics-unsharp = Tiles not yet sharp
diagnostics-language = Language

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
cli-info-path = Path
cli-info-size = Size
cli-info-modified = Modified
cli-info-orientation = Orientation
cli-info-embedded = EXIF thumbnail
cli-info-embedded-at = { $bytes } B at offset { $offset }
cli-info-embedded-none = none
cli-info-quick = Quick preview
cli-info-quick-none = cannot be produced
cli-info-tile = Tile
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

info-rating = Rating
info-label = Label
info-flag = Verdict
info-title = Title
info-description = Description
info-keywords = Keywords
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

diagnostics-selected = Selected

info-taken = Taken
info-taken-none = not yet known
info-dimensions = Dimensions
info-dimensions-px = { $width } × { $height }

cli-info-taken = Taken at
cli-info-taken-none = no date in the file
cli-info-frame = Frame
cli-info-frame-none = no size in the header

cli-info-camera = Camera
cli-info-lens = Lens
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
filter-date = Date
filter-no-rejects = No rejects
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

diagnostics-showing = Showing
command-view-filter = Filter…
command-view-clear-filter = Show everything

filter-any = Any
filter-from = From
filter-to = To
filter-taken-range = this folder spans { $from } to { $to }

task-writing-metadata = Writing into the photographs
diagnostics-unwritten = Waiting to be written
diagnostics-unwritable = Could not be written

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
setting-cache-thumbnails = Keep tiles on disk

## Faces and people

setting-face-models = Face models folder
setting-face-detect-size = Scan decode size (px)
setting-face-crop-size = Face thumbnail decode size (px)
setting-face-crop-margin = Face thumbnail margin
setting-face-frames = Frame the faces over the preview

setting-ai-provider = Who describes (ollama, openai, anthropic, gemini)
setting-ai-endpoint = Ollama address
setting-ai-model = Ollama vision model
setting-ai-api-key = API key of the cloud provider
setting-ai-cloud-endpoint = Cloud provider address (empty for its own)
setting-ai-cloud-model = Cloud vision model
setting-ai-language = Language of the description
setting-ai-overwrite = Overwrite a title and description already there
setting-ai-request-size = Size sent to the model (px)
setting-ai-timeout = Wait for an answer (s)

setting-updates-check = Look for a newer version at start
setting-updates-feed = Where the releases are

## Updates

updates-ready = PhotoSite { $version } is downloaded
updates-restart = Restart into it
diagnostics-updates = Updates
updates-not-installed = not installed, so not asked
updates-off = not asked
updates-asking = asking
updates-current = this is the newest
updates-downloading = downloading { $version }
updates-failed = failed: { $error }

command-photo-people = People…

people-title = People
people-scan = Find faces
people-stop = Stop
people-no-folder = Open a folder first
people-no-models =
    The face models are not in { $folder }. Missing: { $missing }
people-suggestions = Probably somebody you have named
people-is-this = Is this { $name }?
people-yes = Yes
people-no = No
people-groups =
    { $count ->
        [one] { $count } face nobody has named
       *[other] { $count } faces nobody has named
    }
people-group-count =
    { $count ->
        [one] { $count } face
       *[other] { $count } faces
    }
people-new-person = New person…
people-new-name = Name
people-not-a-person = Not somebody
people-known = Known people
people-rename = Rename
people-forget = Forget
people-no-faces = No faces yet
people-click-to-remove = Click a face that is not them
people-and-more = + { $count } more
people-nothing-to-name = No faces yet. Find faces to begin.
people-scoring = Reading expressions

task-faces = Looking for faces
people-swept =
    { $photos ->
        [one] { $photos } photograph
       *[other] { $photos } photographs
    }, { $faces ->
        [one] { $faces } face
       *[other] { $faces } faces
    }
people-recognised =
    { $count ->
        [one] { $count } recognised
       *[other] { $count } recognised
    }
people-to-confirm =
    { $count ->
        [one] { $count } to confirm
       *[other] { $count } to confirm
    }
people-scored =
    { $count ->
        [one] { $count } older face scored
       *[other] { $count } older faces scored
    }
people-unreadable =
    { $count ->
        [one] { $count } could not be read
       *[other] { $count } could not be read
    }

filter-section-people = People
filter-section-expression = Expression
filter-all-smiling = Everyone smiling
filter-someone-not-smiling = Someone not smiling
filter-all-eyes-open = Everyone's eyes open
filter-someone-blinking = Someone blinking

info-people = People
info-expression = Expression
info-expression-smiling = { $count }/{ $of } smiling
info-expression-eyes = { $count }/{ $of } eyes open

cli-faces-done =
    { $photos ->
        [one] swept { $photos } photograph
       *[other] swept { $photos } photographs
    }, { $faces ->
        [one] { $faces } face
       *[other] { $faces } faces
    } in { NUMBER($seconds, maximumFractionDigits: 1) } s
cli-people-total =
    { $people ->
        [one] { $people } person
       *[other] { $people } people
    }, { $named } of { $faces } faces named

cli-named =
    { $faces ->
        [one] named { $faces } face
       *[other] named { $faces } faces
    } as { $name }, on { $photos ->
        [one] { $photos } photograph
       *[other] { $photos } photographs
    }
cli-written =
    wrote { $written }, could not write { $failed }

## Batch conversion

command-photo-batch = Convert…
batch-title = Convert
batch-where = Where
batch-beside-source = Beside each original
batch-into = Into this folder
batch-choose-folder = Choose…
batch-folder-per-day = A folder per day
batch-on-collision = If the name is taken
collision-number = Take a number
collision-skip = Leave it alone
collision-overwrite = Write over it

batch-format = Format
format-same = Keep the format
format-jpeg = JPEG
format-png = PNG
format-webp = WebP
format-tiff = TIFF
format-bmp = BMP
format-webp-lossless = WebP is written lossless here, so a photograph comes out larger than a JPEG, not smaller.
batch-quality = Quality

batch-size = Size
resize-none = Leave the size
resize-width = Width
resize-height = Height
resize-longest = Longest side
resize-shortest = Shortest side
resize-percent = Per cent
batch-allow-enlarging = Enlarge the small ones too
batch-sharpen = Sharpen

batch-naming = Name
naming-original = Keep the name
naming-custom = One name for all
naming-date = Date taken
batch-custom-name = Name
batch-date-tokens = { "{" }year{ "}" } { "{" }month{ "}" } { "{" }day{ "}" } { "{" }hour{ "}" } { "{" }minute{ "}" } { "{" }second{ "}" }
batch-prefix = Prefix
batch-suffix = Suffix
batch-numbering = Number them

batch-carry = Metadata
carry-everything = Carry it all across
carry-without-place = All but the position
carry-nothing = None of it

batch-will-write =
    { $count ->
        [one] { $count } to write
       *[other] { $count } to write
    }
batch-will-skip =
    { $count ->
        [one] { $count } left alone
       *[other] { $count } left alone
    }
batch-will-overwrite =
    { $count ->
        [one] { $count } written over
       *[other] { $count } written over
    }
batch-nowhere = Nowhere to put them yet

batch-run = Convert
batch-stop = Stop
batch-save-preset = Save as a preset
batch-delete-preset = Delete this preset
batch-preset-name = Preset name

task-batch = Converting
batch-written =
    { $count ->
        [one] { $count } written
       *[other] { $count } written
    }
batch-skipped =
    { $count ->
        [one] { $count } left alone
       *[other] { $count } left alone
    }
batch-failed =
    { $count ->
        [one] { $count } failed
       *[other] { $count } failed
    }
batch-cancelled = stopped
batch-exists = the name is taken

cli-batch-plan =
    { $name }: { $write } to write, { $skip } left alone, { $overwrite } written over
cli-batch-done =
    wrote { $written }, left alone { $skipped }, failed { $failed }

compass-north = north
compass-north-east = north-east
compass-east = east
compass-south-east = south-east
compass-south = south
compass-south-west = south-west
compass-west = west
compass-north-west = north-west

## Describing with a model on this machine

command-photo-describe = Describe…
ai-title = Describe
ai-provider = Ask
ai-provider-ollama = Ollama on this machine
ai-provider-openai = OpenAI, or compatible
ai-provider-anthropic = Anthropic
ai-provider-gemini = Google Gemini
ai-endpoint = Ollama at
ai-cloud-endpoint = Address
ai-api-key = API key
ai-api-key-hint = Kept in the settings on this machine; sent to this provider and nowhere else. The photographs go with it.
ai-no-key = This provider needs an API key
ai-model = Model
ai-refresh = Ask again
ai-language = Language
ai-fill-empty = Fill in the empty ones
ai-overwrite = Write over what is there
ai-no-models = The server answered, but offers no models
ai-places-from = Places come from { $source }
ai-no-places = No place list in { $folder }, so no place will be named
ai-run = Describe
ai-stop = Stop
ai-waiting =
    { $count ->
        [one] { $count } photograph
       *[other] { $count } photographs
    }
ai-pace =
    { NUMBER($each, maximumFractionDigits: 0) } s each, about { NUMBER($left, maximumFractionDigits: 0) } min left
ai-described =
    { $count ->
        [one] described { $count } photograph
       *[other] described { $count } photographs
    }
ai-in = in { NUMBER($seconds, maximumFractionDigits: 0) } s
ai-skipped =
    { $count ->
        [one] { $count } already had one
       *[other] { $count } already had one
    }
ai-failed =
    { $count ->
        [one] { $count } failed
       *[other] { $count } failed
    }
ai-all-failing = The first few all failed; check the address and the model.
task-describe = Describing

diagnostics-models = Face models
diagnostics-missing = missing: { $missing }
diagnostics-places = Place list
diagnostics-faces = Faces
diagnostics-faces-of = { $named } of { $faces } named
diagnostics-people = People

## The editor

group-editor = Editor
command-photo-edit = Open
command-editor-close = Close
command-editor-back = Back to the manager on this photograph
command-editor-fullscreen = Fill the screen
command-editor-next = Next photograph
command-editor-previous = Previous photograph
tab-manager = Manager
editor-position = { $at } / { $count }
editor-hint = Wheel or PgUp / PgDn turns the page · Ctrl and the wheel looks closer · middle click or Ctrl+F fills the screen · Enter goes back to the tile · Esc closes
editor-gone = This photograph is no longer in the folder being shown
editor-nothing-chosen = Pick a tile to open it
