# Installing it

```powershell
cd v2
./packaging/pack.ps1
```

Four minutes later there is a `PhotoSite-win-Setup.exe` in
`artifacts/releases`. Double-clicking it puts PhotoSite in
`%LOCALAPPDATA%\PhotoSite`, a shortcut on the desktop and one in the Start
menu, and an entry in *Apps & features* that removes all three again. No
administrator, no prompt, no reboot.

**Read [the one thing to do first](#the-one-thing-to-do-first) before
installing on a machine that ran v1.**

## Velopack, and what it was weighed against

**MSI, through WiX.** The format Windows itself understands, and the only one
a company's deployment tooling will accept. It is also a compiler with its
own XML language, it wants an upgrade GUID and a component GUID per file
managed by hand, and it installs machine-wide, which means the elevation
prompt. All of that buys a thing this application does not need, and buys
nothing towards updating.

**Inno Setup, or NSIS.** Both would produce the installer asked for here, and
produce it well. Neither has anything to say about the second half of the
question: an application that updates itself needs a feed, a version
comparison, a way to swap a folder that is currently running, and a way to
not lose everything when the network drops half way. That is the whole of
what a packaging tool is for, and writing it once for oneself is how it gets
written badly.

**A zip.** Honest, and already produced beside the installer for whoever
wants it — `PHOTOSITE_DATA` makes the whole application portable, so a
folder on a stick genuinely works. It is not an answer for somebody who
wants a program on their computer.

**MSIX.** Wants a signing certificate before it will install at all.

**Velopack** is what v1 shipped with, and the reasons hold better here than
they did there. It needs no certificate, no administrator and no MSI. The
package it installs from *is* the package an update is fetched from, so
there is one artefact and one feed rather than two. It has a Rust SDK, which
is what makes it available at all now that the application is Rust. And the
release it publishes is an ordinary GitHub release, which costs nothing to
host.

Two costs, stated plainly.

**The installer is unsigned**, so SmartScreen shows its blue "Windows
protected your PC" panel until enough people have clicked through it. A code
signing certificate is the only cure and it is an annual bill.
`vpk pack --signParams` is where it would go.

**The window binary grew by 1.9 MB**, 46.9 to 48.8, over fifty crates of
which a whole TLS stack. Fetching a release over HTTPS is what the crate is
for, so that is the price of the feature rather than something dragged in
sideways — but it does change a claim the workspace manifest makes elsewhere.
`ureq` is asked for without TLS on purpose, because Ollama is on localhost;
features unify across a binary, so `photosite-ui` now has rustls whether it
wants it or not. `photosite-cli` does not carry velopack and so is unchanged,
which is also why the note beside `ureq` is worth keeping exactly as it is.

## The one thing to do first

The package is `PhotoSite` and the version starts at `2.0.0`, because this is
PhotoSite — the second of it, and the successor to a v1 that stopped at 0.9.x
and is frozen. Taking the name is the right call and it has one sharp edge.

**Velopack installs into `%LOCALAPPDATA%\<PackId>` and empties that folder
before it writes.** `%LOCALAPPDATA%\PhotoSite` is exactly where v1 kept its
catalogue and its thumbnail cache. On a machine that ran v1 and still has
that folder, installing takes them — at install time, not at uninstall, so
there is no moment at which somebody gets to change their mind. Verified,
not assumed: a folder seeded with a decoy `catalogue.db` and `thumbnails\`
came back holding nothing but `current`, `packages`, `photosite.exe` and
`Update.exe`.

So before the first install on such a machine, move v1's data out of the way:

```powershell
Move-Item "$env:LOCALAPPDATA\PhotoSite" "$env:LOCALAPPDATA\PhotoSite-v1"
```

Nothing is lost by that and v1 is not much harmed either — it hard-wires the
path, so it would build itself a fresh catalogue on the next run, and the
ratings, labels and keywords it would be missing are in the photographs
themselves, where v1 wrote them. What it would genuinely lose is the flags, a
culling session's working state, which were never written to disk anywhere
else.

v2 is not affected in either direction. Its own catalogue lives in
`%APPDATA%\PhotoSite\PhotoSite\data`, which is not the install folder, is not
emptied by an install and survives an uninstall. That separation is why an
update can replace the program without touching the library.

## What is in it

Two executables — `photosite.exe` and `photosite-cli.exe` — and nothing
else. The four ONNX models and the GeoNames extract are fetched on the
machine that wants them, exactly as they are for a build from source: a
hundred and fifty megabytes nobody has asked for yet does not belong in a
first install, and a feature that has not been used has nothing to say about
whether the install worked.

That leaves the download at about 37 MB.

## Cutting a release

The version lives in `v2/Cargo.toml` and nowhere else. A release is:

1. bump `[workspace.package] version`,
2. commit,
3. run the **Release PhotoSite v2** workflow.

It builds, runs the tests again, packs with the same script as above, and
publishes a GitHub release tagged `v<version>` — so `v2.0.0`, and no prefix,
because there is one PhotoSite and this is the next of it. Nothing can
collide: v1 never released anything, and its own workflow would stamp 0.9.x,
which is older than any version this will ever produce.

## Updating itself

An installed PhotoSite asks, five seconds after the window opens and off
the main thread, whether the releases page holds a newer version. When it
does, the package is downloaded into Velopack's `packages` folder while
somebody goes on working; nothing on the screen changes until it is
complete. Then the status row says *PhotoSite 2.0.1 is downloaded* beside a
*Restart into it* button. Whoever does not click gets the new version the
next time PhotoSite starts anyway, because `VelopackApp::run()` applies a
downloaded package before anything else.

Two settings, both under `[updates]`: `check` (on by default) and `feed`,
the repository whose releases page is asked. The feed is a setting rather
than a constant so that a fork can point its installations elsewhere.

The code is `crates/photosite-ui/src/updates.rs`, and it is small on purpose:
a thread, a channel, a state for the diagnostics window and one line of UI.
A build run from source is not installed — no `Update.exe` beside it, no
package manifest — and the thread says so in the log and ends. That is the
ordinary case on a development machine and it is not an error. The
diagnostics window has an *updates* row that says what happened: asked,
current, downloading, downloaded, or why not.

Unauthenticated, the GitHub API allows sixty requests an hour from one
address. One check per start is well within that.
