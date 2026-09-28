//! `Ctrl+V`, heard before the toolkit swallows it.
//!
//! egui-winit does not pass `Ctrl+C`, `Ctrl+X` and `Ctrl+V` on as keys. It
//! turns them into clipboard events of its own — and `Ctrl+V` into nothing at
//! all unless the clipboard holds text. Files copied in Explorer, a picture
//! copied in Zoner: nothing arrives, and the key does nothing. Copy and cut
//! still come through as events and are read as such where the keys are
//! read; paste has to be heard here.
//!
//! On Windows a keyboard hook on the window's own thread notes the press,
//! with the window that was active when it happened. The frame takes the note
//! and acts on it only if that window is still the active one: a `Ctrl+V`
//! typed into a system dialog shown over the window — the program chooser,
//! the system's menu — is that dialog's business, and must not paste into
//! the gallery the moment it closes.
//!
//! Elsewhere there is no hook, and a paste is heard only when the clipboard
//! holds text — which a copy made here always puts there on those systems.

/// Starts listening. Called once, on the thread that runs the window, before
/// the window exists.
pub fn listen() {
    #[cfg(windows)]
    windows_hook::install();
}

/// Whether `Ctrl+V` was pressed in the window since this was last asked.
pub fn take() -> bool {
    #[cfg(windows)]
    {
        windows_hook::take()
    }
    #[cfg(not(windows))]
    {
        false
    }
}

#[cfg(windows)]
mod windows_hook {
    use std::sync::atomic::{AtomicIsize, Ordering};
    use windows::Win32::Foundation::{LPARAM, LRESULT, WPARAM};
    use windows::Win32::System::Threading::GetCurrentThreadId;
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        GetActiveWindow, GetKeyState, VK_CONTROL, VK_MENU, VK_SHIFT, VK_V,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        CallNextHookEx, HC_ACTION, SetWindowsHookExW, WH_KEYBOARD,
    };

    /// The window that was active when `Ctrl+V` was last pressed, or zero.
    static PRESSED_IN: AtomicIsize = AtomicIsize::new(0);

    pub fn install() {
        // SAFETY: a hook on this thread alone, with a procedure that lives as
        // long as the program. It goes when the thread does.
        let hooked =
            unsafe { SetWindowsHookExW(WH_KEYBOARD, Some(hook), None, GetCurrentThreadId()) };
        if let Err(error) = hooked {
            tracing::warn!(%error, "Ctrl+V will be heard only when the clipboard holds text");
        }
    }

    pub fn take() -> bool {
        let pressed_in = PRESSED_IN.swap(0, Ordering::Relaxed);
        // SAFETY: no arguments.
        let active = unsafe { GetActiveWindow() }.0 as isize;
        pressed_in != 0 && pressed_in == active
    }

    fn held(key: windows::Win32::UI::Input::KeyboardAndMouse::VIRTUAL_KEY) -> bool {
        // SAFETY: no pointers; the high bit says whether the key is down.
        unsafe { GetKeyState(i32::from(key.0)) < 0 }
    }

    unsafe extern "system" fn hook(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
        // The flags in the high bits: 31 is set when the key goes up, 30 when
        // it was already down — a key held and repeating is one paste, not
        // twenty.
        let flags = lparam.0 as u32;
        let pressed = flags & (1 << 31) == 0 && flags & (1 << 30) == 0;
        if code == HC_ACTION as i32
            && pressed
            && wparam.0 == usize::from(VK_V.0)
            && held(VK_CONTROL)
            && !held(VK_MENU)
            && !held(VK_SHIFT)
        {
            // SAFETY: no arguments.
            let active = unsafe { GetActiveWindow() }.0 as isize;
            PRESSED_IN.store(active, Ordering::Relaxed);
        }

        // SAFETY: passed on exactly as it came, as every hook must.
        unsafe { CallNextHookEx(None, code, wparam, lparam) }
    }
}
