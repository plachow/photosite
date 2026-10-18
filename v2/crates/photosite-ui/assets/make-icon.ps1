#Requires -Version 5.1
<#
.SYNOPSIS
Rasterizes PhotoSite.svg into PhotoSite.ico and PhotoSite.png, next to it.

.DESCRIPTION
The SVG is the drawing; everything else is derived. The ICO carries 16, 24,
32, 48 and 64 pixel bitmaps and a 256 pixel PNG, which is what Windows,
Explorer and the installer expect of an application icon. The PNG is the
256 pixel rendering on its own, for the window on the platforms that take
an icon as pixels rather than as a resource.

Nothing has to be installed: the SVG is drawn by Microsoft Edge, which every
Windows 10 and 11 has, run headless; the icon file is assembled here, with
System.Drawing reading the renderings back. Each size is drawn at that size
rather than scaled down from a large one, so the small ones stay crisp.

.PARAMETER Edge
Path to msedge.exe, when it is not in one of the usual places.
#>
[CmdletBinding()]
param([string] $Edge)

$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Drawing

# A 32-bit DIB the way an ICO wants it: a header whose height counts the
# mask too, the pixels bottom-up in BGRA, then a one-bit mask of what is
# fully transparent, each of its rows padded to four bytes.
function ConvertTo-IconBitmap([System.Drawing.Bitmap] $bitmap) {
    $width = $bitmap.Width
    $height = $bitmap.Height
    $rect = New-Object System.Drawing.Rectangle 0, 0, $width, $height
    $data = $bitmap.LockBits($rect, [System.Drawing.Imaging.ImageLockMode]::ReadOnly,
        [System.Drawing.Imaging.PixelFormat]::Format32bppArgb)
    try {
        $stride = $data.Stride
        $rows = New-Object byte[] ($stride * $height)
        [System.Runtime.InteropServices.Marshal]::Copy($data.Scan0, $rows, 0, $rows.Length)
    } finally {
        $bitmap.UnlockBits($data)
    }

    $maskStride = [int][math]::Ceiling($width / 32.0) * 4
    $stream = New-Object System.IO.MemoryStream
    $w = New-Object System.IO.BinaryWriter $stream
    $w.Write([uint32] 40)                        # BITMAPINFOHEADER
    $w.Write([int32] $width)
    $w.Write([int32] ($height * 2))              # pixels and mask
    $w.Write([uint16] 1)                         # planes
    $w.Write([uint16] 32)                        # bits per pixel
    $w.Write([uint32] 0)                         # BI_RGB
    $w.Write([uint32] ($width * 4 * $height + $maskStride * $height))
    $w.Write([int32] 0); $w.Write([int32] 0)     # pixels per metre
    $w.Write([uint32] 0); $w.Write([uint32] 0)   # colours used, important

    for ($y = $height - 1; $y -ge 0; $y--) {
        $w.Write($rows, $y * $stride, $width * 4)
    }
    for ($y = $height - 1; $y -ge 0; $y--) {
        $mask = New-Object byte[] $maskStride
        for ($x = 0; $x -lt $width; $x++) {
            $alpha = $rows[$y * $stride + $x * 4 + 3]
            if ($alpha -eq 0) {
                $mask[$x -shr 3] = $mask[$x -shr 3] -bor (0x80 -shr ($x -band 7))
            }
        }
        $w.Write($mask)
    }
    $w.Flush()
    return ,$stream.ToArray()
}

$here  = $PSScriptRoot
$svg   = Join-Path $here 'PhotoSite.svg'
$ico   = Join-Path $here 'PhotoSite.ico'
$png   = Join-Path $here 'PhotoSite.png'
$sizes = 16, 24, 32, 48, 64, 256
$gap   = 8

if (-not $Edge) {
    $Edge = @(
        "$env:ProgramFiles\Microsoft\Edge\Application\msedge.exe",
        "${env:ProgramFiles(x86)}\Microsoft\Edge\Application\msedge.exe"
    ) | Where-Object { Test-Path $_ } | Select-Object -First 1
    if (-not $Edge) {
        $Edge = (Get-Command msedge.exe -ErrorAction SilentlyContinue).Source
    }
}
if (-not $Edge) { throw 'Microsoft Edge was not found; pass -Edge <path to msedge.exe>.' }

# One page with every size side by side, left to right, each image rendered
# at its own pixel size, then cut apart. A single screenshot is cheaper than
# six and sidesteps the smallest window a browser is willing to open.
$work = Join-Path ([IO.Path]::GetTempPath()) ('photosite-icon-' + [guid]::NewGuid())
New-Item -ItemType Directory $work | Out-Null
try {
    $width = ($sizes | Measure-Object -Sum).Sum + $gap * ($sizes.Count - 1)
    $source = ([Uri] $svg).AbsoluteUri
    $spacer = "<span style=`"display:inline-block;width:${gap}px`"></span>"
    $images = ($sizes | ForEach-Object { "<img src=`"$source`" width=`"$_`" height=`"$_`">" }) -join $spacer
    $html = Join-Path $work 'icon.html'
    Set-Content -Encoding utf8 -Path $html -Value @"
<!doctype html><html><head><meta charset="utf-8"><style>
html,body{margin:0;background:transparent;overflow:hidden;white-space:nowrap;font-size:0;line-height:0}
img{vertical-align:top}
</style></head><body>$images</body></html>
"@

    $sheet = Join-Path $work 'sheet.png'
    Write-Host "==> drawing $svg with Edge" -ForegroundColor Cyan
    # Edge reports the screenshot on stderr, which Windows PowerShell would
    # take for an error; it goes to a file instead, and is read back only
    # when there is no screenshot to show for it.
    $arguments = @(
        '--headless=new', '--disable-gpu', '--hide-scrollbars', '--no-first-run', '--disable-extensions',
        "--user-data-dir=`"$work\profile`"", '--allow-file-access-from-files',
        '--default-background-color=00000000', '--virtual-time-budget=5000',
        "--window-size=$width,256", "--screenshot=`"$sheet`"", ([Uri] $html).AbsoluteUri
    )
    $said = Join-Path $work 'edge.txt'
    $edgeRun = Start-Process -FilePath $Edge -ArgumentList $arguments -Wait -PassThru -NoNewWindow `
        -RedirectStandardError $said
    if (-not (Test-Path $sheet)) {
        $why = if (Test-Path $said) { Get-Content $said -Raw } else { '' }
        throw "Edge produced no screenshot in $work (exit code $($edgeRun.ExitCode)). $why"
    }

    $entries = @()
    $whole = [System.Drawing.Bitmap]::FromFile($sheet)
    try {
        if ($whole.Width -lt $width -or $whole.Height -lt 256) {
            throw "The screenshot is $($whole.Width)x$($whole.Height), smaller than the ${width}x256 asked for."
        }
        $x = 0
        foreach ($size in $sizes) {
            $rect = New-Object System.Drawing.Rectangle $x, 0, $size, $size
            $tile = $whole.Clone($rect, [System.Drawing.Imaging.PixelFormat]::Format32bppArgb)
            $x += $size + $gap
            try {
                if ($tile.GetPixel(0, 0).A -ne 0) {
                    throw "The $size pixel rendering has an opaque corner; the page background did not come out transparent."
                }
                if ($size -eq 256) {
                    $stream = New-Object System.IO.MemoryStream
                    $tile.Save($stream, [System.Drawing.Imaging.ImageFormat]::Png)
                    $bytes = $stream.ToArray()
                    [IO.File]::WriteAllBytes($png, $bytes)
                } else {
                    $bytes = ConvertTo-IconBitmap $tile
                }
            } finally {
                $tile.Dispose()
            }
            $entries += [pscustomobject]@{ Size = $size; Bytes = $bytes }
        }
    } finally {
        $whole.Dispose()
    }

    Write-Host "==> writing $ico" -ForegroundColor Cyan
    $out = New-Object System.IO.MemoryStream
    $w = New-Object System.IO.BinaryWriter $out
    $w.Write([uint16] 0)                 # reserved
    $w.Write([uint16] 1)                 # 1 = icon
    $w.Write([uint16] $entries.Count)
    $offset = 6 + 16 * $entries.Count
    foreach ($entry in $entries) {
        $side = if ($entry.Size -ge 256) { 0 } else { $entry.Size }
        $w.Write([byte] $side)           # width, 0 meaning 256
        $w.Write([byte] $side)           # height
        $w.Write([byte] 0)               # colours in palette: none
        $w.Write([byte] 0)               # reserved
        $w.Write([uint16] 1)             # colour planes
        $w.Write([uint16] 32)            # bits per pixel
        $w.Write([uint32] $entry.Bytes.Length)
        $w.Write([uint32] $offset)
        $offset += $entry.Bytes.Length
    }
    foreach ($entry in $entries) { $w.Write([byte[]] $entry.Bytes) }
    $w.Flush()
    [IO.File]::WriteAllBytes($ico, $out.ToArray())
    Write-Host ("    {0} entries: {1}; {2} bytes" -f $entries.Count, (($entries | ForEach-Object { $_.Size }) -join ', '), $out.Length)
} finally {
    Remove-Item -Recurse -Force $work -ErrorAction SilentlyContinue
}
