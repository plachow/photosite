//! What has to happen before anything else, and is invisible when it works.
//!
//! Both of the things here are consequences of the application now being
//! *installed* rather than only ever started with `cargo run`.

/// Runs before the settings are read, before the log is opened and before a
/// window exists.
///
/// The order matters and is not a matter of taste. The installer starts this
/// executable to make its shortcuts, and the updater starts it again to swap
/// the folder underneath itself; on both of those runs
/// [`velopack::VelopackApp::run`] does its work and ends the process. Doing
/// anything first — creating folders, opening the catalogue, putting a window
/// on the screen — would mean doing it during an install nobody is watching.
pub fn first() {
    attach_console();
    velopack::VelopackApp::build().run();
}

/// Lends the executable the console that started it, when it has nowhere
/// else to write.
///
/// The binary is linked for the windowed subsystem, because an installed
/// application that puts a black rectangle on the screen beside its own
/// window looks broken and is. The cost is that `println!` then writes to a
/// handle nobody is holding, and `--selftest` says nothing.
///
/// The test is on the handle rather than on the console, and that is the
/// whole of it. Attaching unconditionally *replaces* the standard handles
/// with the console's, so `photosite --selftest > verdict.txt` wrote the
/// file and left it empty — the one use of the switch that a script has.
/// Whoever started us has usually handed over a handle already, whether a
/// file, a pipe or their own console, and that handle is writable without
/// being attached to anything. Only when there is none — a shortcut, the
/// installer, the updater — is there anything to borrow, and then the borrow
/// fails harmlessly because there is no parent console either.
#[cfg(windows)]
fn attach_console() {
    use windows_sys::Win32::Foundation::INVALID_HANDLE_VALUE;
    use windows_sys::Win32::System::Console::{
        ATTACH_PARENT_PROCESS, AttachConsole, GetStdHandle, STD_OUTPUT_HANDLE,
    };

    // SAFETY: both take no pointers, and the two failures — no handle, no
    // parent console — are the ones being asked about. It has to happen
    // before the first print, because the standard handles are looked up
    // once and then kept.
    unsafe {
        let out = GetStdHandle(STD_OUTPUT_HANDLE);
        if out.is_null() || out == INVALID_HANDLE_VALUE {
            AttachConsole(ATTACH_PARENT_PROCESS);
        }
    }
}

#[cfg(not(windows))]
fn attach_console() {}
