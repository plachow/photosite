//! The picture on the window: in the title bar, on the task bar, in the
//! Alt+Tab switcher.
//!
//! The drawing is `assets/PhotoSite.svg`; everything else is rasterized
//! from it by `assets/make-icon.ps1`. The executable carries the result as
//! a Windows icon resource (see `build.rs`), which is what Explorer, the
//! Start menu and the installer show. The window has to be told separately:
//! a window that says nothing about its icon gets the system's blank one,
//! because no platform goes looking in the executable on its behalf.
//!
//! So the window is handed the drawing as pixels, which is the one form
//! every platform accepts. On Windows that is only the first word: winit
//! turns the pixels into a single bitmap, and the system then scales that
//! bitmap to the title bar and to the task bar as it sees fit, which at 16
//! pixels looks like a photocopy of a photocopy. The icon resource holds a
//! drawing made for each of those sizes, so once the window exists it is
//! asked to use those instead.

use eframe::egui;
use std::sync::Arc;

/// The window's icon as pixels: the 256-pixel rendering.
///
/// A PNG that cannot be read is a build that went wrong, not something to
/// stop the application over; the window then keeps the system's icon,
/// and the log says why.
pub fn data() -> Arc<egui::IconData> {
    let png = include_bytes!("../assets/PhotoSite.png");
    match eframe::icon_data::from_png_bytes(png) {
        Ok(icon) => Arc::new(icon),
        Err(error) => {
            tracing::warn!("the window icon could not be read: {error}");
            Arc::new(egui::IconData::default())
        }
    }
}

/// Once the window exists: on Windows, swap the scaled bitmap for the sizes
/// the executable carries. Elsewhere the pixels are all there is to give.
pub fn adopt(cc: &eframe::CreationContext<'_>) {
    #[cfg(windows)]
    windows_icon::adopt(cc);
    #[cfg(not(windows))]
    let _ = cc;
}

#[cfg(windows)]
mod windows_icon {
    // raw-window-handle is not a dependency of this crate by name; it is
    // what the window handle of a `CreationContext` is expressed in, and
    // wgpu — which eframe re-exports — hands the crate on as `rwh`.
    use eframe::wgpu::rwh::{HasWindowHandle, RawWindowHandle};
    use windows::Win32::Foundation::{HINSTANCE, HWND, LPARAM, WPARAM};
    use windows::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows::Win32::UI::WindowsAndMessaging::{
        GetSystemMetrics, ICON_BIG, ICON_SMALL, IMAGE_ICON, LR_DEFAULTCOLOR, LoadImageW,
        SM_CXICON, SM_CXSMICON, SM_CYICON, SM_CYSMICON, SendMessageW, WM_SETICON,
    };
    use windows::core::PCWSTR;

    /// The number `build.rs` files the icon under: `set_icon` gives it 1.
    const ICON_RESOURCE: usize = 1;

    pub fn adopt(cc: &eframe::CreationContext<'_>) {
        let Ok(handle) = cc.window_handle() else {
            return;
        };
        let RawWindowHandle::Win32(handle) = handle.as_raw() else {
            return;
        };
        let hwnd = HWND(handle.hwnd.get() as *mut _);
        let module = match handle.hinstance {
            Some(instance) => HINSTANCE(instance.get() as *mut _),
            // SAFETY: no name means the executable itself; no pointers go in.
            None => match unsafe { GetModuleHandleW(None) } {
                Ok(module) => module.into(),
                Err(error) => {
                    tracing::debug!("no module to take the window icon from: {error}");
                    return;
                }
            },
        };

        // The small one is the title bar's; the big one is the task bar's
        // and the switcher's. Asked for at the sizes the system wants them
        // at on this screen, so that the resource's 16 (or 24, or 32) is
        // used as drawn rather than scaled from a neighbour.
        for (which, width, height) in [
            (ICON_SMALL, SM_CXSMICON, SM_CYSMICON),
            (ICON_BIG, SM_CXICON, SM_CYICON),
        ] {
            // SAFETY: the window is ours and alive — the frame has not
            // been painted yet — and the icon handle is shared with the
            // system for as long as the window lives, which is for as long
            // as the process does.
            unsafe {
                let loaded = LoadImageW(
                    Some(module),
                    PCWSTR(ICON_RESOURCE as *const u16),
                    IMAGE_ICON,
                    GetSystemMetrics(width),
                    GetSystemMetrics(height),
                    LR_DEFAULTCOLOR,
                );
                match loaded {
                    Ok(icon) => {
                        SendMessageW(
                            hwnd,
                            WM_SETICON,
                            Some(WPARAM(which as usize)),
                            Some(LPARAM(icon.0 as isize)),
                        );
                    }
                    // A build without the resource compiler leaves the
                    // executable without the resource; the pixels winit was
                    // given stay, and that is the right answer.
                    Err(error) => {
                        tracing::debug!("the executable carries no icon to show: {error}");
                        return;
                    }
                }
            }
        }
    }
}
