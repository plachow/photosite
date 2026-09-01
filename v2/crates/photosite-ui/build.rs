//! What Explorer, the task bar and the shortcut show.
//!
//! An executable with no resources of its own gets the toolchain's blank
//! icon and a properties page saying nothing. That is tolerable while the
//! only way to start the application is `cargo run`; the moment it is
//! installed, the icon is the whole of how somebody finds it again.
//!
//! The icon is the same drawing v1 rasterized, kept here so that the build
//! script and the installer point at one file rather than at two copies that
//! will one day differ.

fn main() {
    println!("cargo:rerun-if-changed=assets/PhotoSite.ico");

    // The target, not the host: this file is compiled for the machine doing
    // the building, and asking `cfg!(windows)` here would embed a Windows
    // resource into a Linux binary whenever somebody cross-compiles.
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }

    let mut resource = winresource::WindowsResource::new();
    resource.set_icon("assets/PhotoSite.ico");
    // Version and company come from Cargo.toml on their own. These two do
    // not: the defaults would be the crate name, and `photosite-ui` is an
    // implementation detail nobody outside this repository should be shown.
    resource.set("ProductName", "PhotoSite");
    resource.set("FileDescription", "PhotoSite");
    resource.set("CompanyName", "plachow");

    if let Err(error) = resource.compile() {
        // Not fatal. A cross-build, or a Windows machine without the SDK's
        // resource compiler, should still produce a working binary — one
        // that merely looks unfinished. Refusing to build over an icon
        // would be the wrong trade.
        println!("cargo:warning=the icon could not be embedded: {error}");
    }
}
