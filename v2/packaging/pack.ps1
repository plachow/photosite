<#
.SYNOPSIS
    Builds PhotoSite and packs it into a Windows installer.

.DESCRIPTION
    Produces, in v2/artifacts/releases:

        PhotoSite2-win-Setup.exe        what somebody double-clicks
        PhotoSite2-win-Portable.zip     the same thing, unpacked, for a stick
        PhotoSite2-<version>-full.nupkg what an update is later fetched from
        releases.win.json               the feed that says which is newest

    Nothing here needs an administrator, a certificate or a machine-wide
    install. The same three commands run in CI; see
    .github/workflows/v2-release.yml.

.PARAMETER Version
    The version to stamp. Without one, the workspace's own — Cargo.toml is
    the single source of truth and a release is a version bump plus this
    script.

.PARAMETER OutputDir
    Where the packages go. Defaults to v2/artifacts/releases.

.PARAMETER SkipBuild
    Pack what is already in target/release. For trying the packaging itself
    without waiting four minutes for a build that has not changed.

.PARAMETER Fresh
    Empty the output directory first. Without it, packing a version that is
    already there fails — which is what a release build wants, because the
    previous release has to be sitting in that folder for this one to be a
    delta against it. Trying the packaging twice at the same version is the
    other case, and this is it.
#>
[CmdletBinding()]
param(
    [string] $Version,
    [string] $OutputDir,
    [switch] $SkipBuild,
    [switch] $Fresh
)

$ErrorActionPreference = 'Stop'

# The identity of the package, and the one decision here worth explaining.
# Velopack installs into %LOCALAPPDATA%\<PackId> and its uninstaller removes
# that folder whole. %LOCALAPPDATA%\PhotoSite is where v1 keeps its
# catalogue, so an application called PhotoSite would install on top of it and
# an uninstall would take a hundred megabytes of somebody's work with it.
# Until v2 has the editor and genuinely replaces v1, it is a second
# application that happens to share a name on its shortcut.
$PackId     = 'PhotoSite2'
$PackTitle  = 'PhotoSite'
$PackAuthor = 'plachow'
$VpkVersion = '1.2.0'

$v2   = Split-Path -Parent $PSScriptRoot
$icon = Join-Path $v2 'crates/photosite-ui/assets/PhotoSite.ico'

function Invoke-Checked {
    param([string] $What, [scriptblock] $Do)
    Write-Host "==> $What" -ForegroundColor Cyan
    & $Do
    if ($LASTEXITCODE -ne 0) { throw "$What failed with exit code $LASTEXITCODE." }
}

if (-not $Version) {
    # From the build system rather than from a regular expression over
    # Cargo.toml: the workspace inherits its version and a hand-rolled parse
    # would read the wrong line the day that changes.
    $meta = cargo metadata --manifest-path (Join-Path $v2 'Cargo.toml') --format-version 1 --no-deps | ConvertFrom-Json
    if ($LASTEXITCODE -ne 0) { throw 'cargo metadata failed; is cargo on PATH?' }
    $Version = ($meta.packages | Where-Object name -eq 'photosite-ui').version
}

if ($Version -notmatch '^\d+\.\d+\.\d+$') {
    throw "'$Version' is not a version this can release."
}

if (-not $OutputDir) { $OutputDir = Join-Path $v2 'artifacts/releases' }
$stage = Join-Path $v2 'artifacts/stage'

if ($Fresh -and (Test-Path $OutputDir)) { Remove-Item $OutputDir -Recurse -Force }

if (-not $SkipBuild) {
    Invoke-Checked 'Building' {
        cargo build --release --manifest-path (Join-Path $v2 'Cargo.toml') `
            -p photosite-ui -p photosite-cli
    }
}

# Emptied rather than added to. Whatever is in here is what gets installed,
# so a file left behind by an earlier run would be shipped without anybody
# deciding to.
if (Test-Path $stage) { Remove-Item $stage -Recurse -Force }
New-Item -ItemType Directory -Path $stage | Out-Null

# Two executables and nothing else. The face models and the gazetteer are
# fetched on the machine that wants them — a hundred and fifty megabytes
# nobody has asked for yet does not belong in a first install.
foreach ($exe in 'photosite.exe', 'photosite-cli.exe') {
    $from = Join-Path $v2 "target/release/$exe"
    if (-not (Test-Path $from)) { throw "$from is not there; build first." }
    Copy-Item $from $stage
}

Invoke-Checked "Packing $PackTitle $Version" {
    dnx "vpk@$VpkVersion" --yes -- pack `
        --packId      $PackId `
        --packTitle   $PackTitle `
        --packAuthors $PackAuthor `
        --packVersion $Version `
        --packDir     $stage `
        --mainExe     'photosite.exe' `
        --icon        $icon `
        --outputDir   $OutputDir
}

Write-Host ''
Write-Host "$PackTitle $Version is in $OutputDir" -ForegroundColor Green
foreach ($file in Get-ChildItem $OutputDir | Sort-Object Name) {
    Write-Host ('  {0,10:N1} MB  {1}' -f ($file.Length / 1MB), $file.Name)
}
