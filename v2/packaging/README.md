# Installing it

```powershell
cd v2
./packaging/pack.ps1
```

Four minutes later there is a `PhotoSite2-win-Setup.exe` in
`artifacts/releases`. Double-clicking it puts PhotoSite in
`%LOCALAPPDATA%\PhotoSite2`, a shortcut on the desktop and one in the Start
menu, and an entry in *Apps & features* that removes all three again. No
administrator, no prompt, no reboot.

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

## Why the package is called PhotoSite2

Velopack installs into `%LOCALAPPDATA%\<PackId>` and its uninstaller removes
that folder whole. `%LOCALAPPDATA%\PhotoSite` is where **v1 keeps its
catalogue** — a hundred megabytes of somebody's culling. An application
called `PhotoSite` would install on top of it and one uninstall would take
it.

So v2 installs beside v1 rather than over it, which is what it is: a second
application, with its own catalogue in its own folder, that does not yet have
the editor. The shortcut still says PhotoSite, because `packTitle` is what a
person reads and `packId` is what the file system does. When v2 has the
editor and genuinely replaces v1, taking the name back is one line in
[`pack.ps1`](pack.ps1) and a fresh install.

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
publishes a GitHub release tagged `v2-<version>`. The tag is prefixed because
v1's releases are `v<version>` in the same repository and the two must not be
mistaken for one another.

## Updating itself

Not yet, and the shape of what is missing is small.

The application already calls `VelopackApp::run()` before anything else — it
has to, because that is what the installer and the updater invoke to make
shortcuts and to swap the folder — and that call also *applies* an update
that has already been downloaded, on the next start. What has no code yet is
the half that asks: an `UpdateManager` over
`sources::GithubSource`, a check that does not happen on the main thread, and
somewhere in the window to say that a new version is there. It is a setting,
a task and a line of UI, and it is deliberately not being written before
there is a release to update *from*.
