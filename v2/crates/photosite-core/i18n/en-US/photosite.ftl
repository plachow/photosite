# PhotoSite — en-US
#
# Zdrojový jazyk a zároveň záloha: co v jiném překladu chybí, se vezme odsud.
# Klíče se nikdy nemění ani nepřejmenovávají, jen přibývají — na cizích
# discích jsou podle nich složené překlady.

## Skupiny příkazů

group-file = File
group-view = View
group-help = Help

## Příkazy

command-file-open-folder = Open folder…
command-file-rescan = Reload folder
command-file-quit = Quit
command-view-recursive = Include subfolders
command-view-bigger-tiles = Larger tiles
command-view-smaller-tiles = Smaller tiles
command-view-next-theme = Next theme
command-help-diagnostics = Diagnostics

## Motivy

theme-dark = Dark grey
theme-light = Light grey
theme-sepia = Sepia

## Mřížka

toolbar-recursive = Recursive
gallery-empty = No photographs here
gallery-pick-folder = Pick a folder on the left
preview-pick-tile = Click a tile to see it here

# Plurál je tu schválně: čeština bude potřebovat tři tvary a formát, který
# to neumí, by se musel později přepisovat i s voláními.
gallery-count =
    { $count ->
        [one] { $count } photograph
       *[other] { $count } photographs
    } in { NUMBER($ms, maximumFractionDigits: 0) } ms

## Diagnostika

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

## Samokontrola

selftest-ok =
    self-check passed: { $count } photographs, no blank tile, { NUMBER($ms, maximumFractionDigits: 0) } ms
selftest-no-photos = SELF-CHECK FAILED: the folder holds no photographs
selftest-blank =
    SELF-CHECK FAILED: { $count } tiles still blank after { NUMBER($seconds, maximumFractionDigits: 0) } s

## Příkazová řádka

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

## Chyby

error-not-a-folder = { $path } is not a folder
error-not-a-file = { $path } is not a file
