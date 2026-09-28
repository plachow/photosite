//! Windows' own shell: which programs open a photograph, and the menu
//! Explorer would show for it.
//!
//! Both are questions only the system can answer. The list of programs is
//! whatever Explorer's *Open with* would offer — an editor installed last
//! week is on it without anybody telling us — and the system menu is every
//! verb every installed program has added, *Send to* and *Properties* and
//! the rest. Rebuilding either here would be a poorer copy that is out of
//! date the day something is installed.
//!
//! Three things in here are not obvious and each one is a menu that looks
//! broken without it:
//!
//! * **A menu the shell fills is owner-drawn in places**, and its submenus
//!   are filled only as they open. The messages that ask for that go to
//!   whichever window owns the menu, and they have to be handed back to the
//!   shell or *Send to* opens empty. The application's window belongs to
//!   winit and is not ours to reach into, so the menu is owned by a small
//!   hidden window of our own that does nothing else.
//! * **A menu owned by a window that is not in front does not close** when
//!   somebody clicks elsewhere. The hidden window is brought forward first,
//!   and the application's own window afterwards.
//! * **A COM object is not kept between frames.** The list of programs is
//!   kept by name, and the program chosen is found again by that name when
//!   it is clicked.
//!
//! Elsewhere than on Windows every one of these says so and does nothing.

use std::path::PathBuf;

/// A program that says it opens a kind of file.
///
/// Made only where there is a shell to ask; elsewhere the list is empty.
#[cfg_attr(not(windows), allow(dead_code))]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Handler {
    /// What it is called on screen.
    pub title: String,
    /// What the system calls it: the program's path, or an app's id. How the
    /// same one is found again when it is chosen.
    pub name: String,
}

/// What was chosen from the system menu. Only Windows has one to choose
/// from.
#[cfg_attr(not(windows), allow(dead_code))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Chosen {
    /// Nothing at all, or something the system has already done.
    Nothing,
    /// Something the application does itself. Deleting and renaming go
    /// through our own commands so the catalogue follows the file — the
    /// shell's rename does nothing at all outside Explorer, and its delete
    /// would leave the stars behind in a row nobody can reach.
    Command(&'static str),
}

/// The photographs one menu can be about: those in the same folder as the
/// first.
///
/// The shell asks a folder about its own files, so one menu cannot speak
/// for files in two folders — which a view that includes the subfolders
/// can easily have selected.
pub fn one_folder(paths: &[PathBuf]) -> Vec<PathBuf> {
    let Some(folder) = paths.first().and_then(|first| first.parent()) else {
        return Vec::new();
    };

    paths
        .iter()
        .filter(|path| path.parent() == Some(folder))
        .cloned()
        .collect()
}

/// Which of our own commands does what the shell calls this verb.
#[cfg_attr(not(windows), allow(dead_code))]
fn ours(verb: &str) -> Option<&'static str> {
    match verb.to_ascii_lowercase().as_str() {
        "delete" => Some("file.delete"),
        "rename" => Some("file.rename"),
        "copy" => Some("file.copy"),
        "cut" => Some("file.cut"),
        _ => None,
    }
}

/// Is the system's own menu something this platform has?
pub const AVAILABLE: bool = cfg!(windows);

#[cfg(windows)]
pub use windows_shell::{choose_app, handlers, open_with, system_menu};

#[cfg(not(windows))]
pub fn handlers(_extension: &str) -> Vec<Handler> {
    Vec::new()
}

#[cfg(not(windows))]
pub fn open_with(_handler: &Handler, _extension: &str, _paths: &[PathBuf]) -> anyhow::Result<()> {
    anyhow::bail!("{}", photosite_core::t!("files-windows-only"))
}

#[cfg(not(windows))]
pub fn choose_app(_path: &std::path::Path) -> anyhow::Result<()> {
    anyhow::bail!("{}", photosite_core::t!("files-windows-only"))
}

#[cfg(not(windows))]
pub fn system_menu(_paths: &[PathBuf]) -> anyhow::Result<Chosen> {
    anyhow::bail!("{}", photosite_core::t!("files-windows-only"))
}

#[cfg(windows)]
mod windows_shell {
    use super::{Chosen, Handler, ours};
    use anyhow::{Context, Result};
    use std::cell::RefCell;
    use std::path::{Path, PathBuf};
    use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, WPARAM};
    use windows::Win32::System::Com::{
        COINIT_APARTMENTTHREADED, CoInitializeEx, CoTaskMemFree, CoUninitialize, IDataObject,
    };
    use windows::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        GetActiveWindow, GetKeyState, VK_CONTROL, VK_SHIFT,
    };
    use windows::Win32::UI::Shell::Common::ITEMIDLIST;
    use windows::Win32::UI::Shell::{
        ASSOC_FILTER_RECOMMENDED, CMF_EXTENDEDVERBS, CMF_NORMAL, CMIC_MASK_CONTROL_DOWN,
        CMIC_MASK_PTINVOKE, CMIC_MASK_SHIFT_DOWN, CMINVOKECOMMANDINFO, CMINVOKECOMMANDINFOEX,
        GCS_VERBW, IAssocHandler, IContextMenu, IContextMenu2, IContextMenu3, ILFindLastID,
        IShellFolder, OAIF_EXEC, OPENASINFO, SHAssocEnumHandlers, SHBindToParent, SHOpenWithDialog,
        SHParseDisplayName,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        CreatePopupMenu, CreateWindowExW, DefWindowProcW, DestroyMenu, DestroyWindow, GetCursorPos,
        PostMessageW, RegisterClassW, SW_SHOWNORMAL, SetForegroundWindow, TPM_RETURNCMD,
        TPM_RIGHTBUTTON, TrackPopupMenuEx, WINDOW_EX_STYLE, WM_DRAWITEM, WM_INITMENUPOPUP,
        WM_MEASUREITEM, WM_MENUCHAR, WM_NULL, WNDCLASSW, WS_POPUP,
    };
    use windows::core::{HSTRING, Interface, PCSTR, PCWSTR, PSTR, PWSTR, w};

    /// `CMIC_MASK_UNICODE`, which the bindings do not carry: the wide
    /// fields of the invocation are the ones to read.
    const CMIC_MASK_UNICODE: u32 = 0x0000_4000;
    /// The command numbers the shell may use, and the first of them. Zero is
    /// what the menu answers when nothing was chosen, so it cannot be one.
    const FIRST: u32 = 1;
    const LAST: u32 = 0x7FFF;

    /// COM, for as long as this lives.
    ///
    /// The window's thread already has it — winit starts OLE there for drag
    /// and drop — and then this only counts one more. Released only when it
    /// was ours to release.
    struct Com(bool);

    impl Com {
        fn start() -> Self {
            // SAFETY: no reserved pointer; the matching release is in Drop.
            let started = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) };
            Self(started.is_ok())
        }
    }

    impl Drop for Com {
        fn drop(&mut self) {
            if self.0 {
                // SAFETY: balanced with a successful CoInitializeEx above.
                unsafe { CoUninitialize() };
            }
        }
    }

    /// An absolute item ID list, freed when it goes.
    struct Pidl(*mut ITEMIDLIST);

    impl Drop for Pidl {
        fn drop(&mut self) {
            if !self.0.is_null() {
                // SAFETY: allocated by SHParseDisplayName with the COM
                // allocator, which is what ILFree is.
                unsafe { CoTaskMemFree(Some(self.0 as *const _)) };
            }
        }
    }

    fn pidl(path: &Path) -> Result<Pidl> {
        let mut pidl: *mut ITEMIDLIST = std::ptr::null_mut();
        // SAFETY: the name outlives the call; the list is ours to free.
        unsafe { SHParseDisplayName(&HSTRING::from(path), None, &mut pidl, 0, None) }
            .with_context(|| format!("the shell does not know {}", path.display()))?;
        Ok(Pidl(pidl))
    }

    /// The window dialogs raised by the shell belong to: ours, when it is
    /// the one in front.
    fn owner() -> HWND {
        // SAFETY: no arguments; a null window is a valid answer.
        unsafe { GetActiveWindow() }
    }

    /// The shell's own object for these files — a data object to hand to a
    /// program, or the menu Explorer would show.
    fn ui_object<T: Interface>(paths: &[PathBuf], owner: HWND) -> Result<T> {
        anyhow::ensure!(!paths.is_empty(), "nothing to ask the shell about");
        let lists = paths
            .iter()
            .map(|path| pidl(path))
            .collect::<Result<Vec<_>>>()?;

        // SAFETY: every list stays alive until after GetUIObjectOf, which
        // copies what it keeps.
        unsafe {
            let folder: IShellFolder = SHBindToParent(lists[0].0, None)?;
            let children: Vec<*const ITEMIDLIST> = lists
                .iter()
                .map(|list| ILFindLastID(list.0) as *const ITEMIDLIST)
                .collect();
            Ok(folder.GetUIObjectOf::<T>(owner, &children, None)?)
        }
    }

    /// A string the shell allocated, taken and freed.
    fn taken(text: PWSTR) -> String {
        // SAFETY: a null-terminated string the callee allocated with the COM
        // allocator and handed to us to free.
        unsafe {
            let owned = text.to_string().unwrap_or_default();
            CoTaskMemFree(Some(text.0 as *const _));
            owned
        }
    }

    fn each_handler(extension: &str, mut visit: impl FnMut(IAssocHandler, String, String) -> bool) {
        let dotted = HSTRING::from(format!(".{extension}"));
        // SAFETY: the extension outlives the call.
        let Ok(list) = (unsafe { SHAssocEnumHandlers(&dotted, ASSOC_FILTER_RECOMMENDED) }) else {
            return;
        };

        loop {
            let mut one: [Option<IAssocHandler>; 1] = [None];
            let mut fetched = 0u32;
            // SAFETY: room for exactly one, and the count says whether it
            // came.
            if unsafe { list.Next(&mut one, Some(&mut fetched)) }.is_err() || fetched == 0 {
                break;
            }

            let Some(handler) = one[0].take() else {
                break;
            };
            // SAFETY: both strings are ours to free, which `taken` does.
            let (title, name) = unsafe { (handler.GetUIName(), handler.GetName()) };
            let (Ok(title), Ok(name)) = (title, name) else {
                continue;
            };
            if !visit(handler, taken(title), taken(name)) {
                break;
            }
        }
    }

    /// The programs Explorer would offer to open this kind of file with.
    pub fn handlers(extension: &str) -> Vec<Handler> {
        let _com = Com::start();
        let mut found: Vec<Handler> = Vec::new();
        each_handler(extension, |_, title, name| {
            if !title.is_empty() && !found.iter().any(|already| already.name == name) {
                found.push(Handler { title, name });
            }
            true
        });
        found
    }

    /// Hands the photographs to a program.
    pub fn open_with(handler: &Handler, extension: &str, paths: &[PathBuf]) -> Result<()> {
        let _com = Com::start();
        let mut chosen: Option<IAssocHandler> = None;
        each_handler(extension, |candidate, _, name| {
            if name == handler.name {
                chosen = Some(candidate);
                false
            } else {
                true
            }
        });

        let chosen = chosen.with_context(|| format!("{} no longer opens these", handler.title))?;
        let data: IDataObject = ui_object(paths, owner())?;
        // SAFETY: the data object is alive for the call.
        unsafe { chosen.Invoke(&data) }
            .with_context(|| format!("{} would not open them", handler.title))
    }

    /// The system's own dialog for choosing a program. A dialog closed
    /// without choosing is an answer like any other.
    pub fn choose_app(path: &Path) -> Result<()> {
        let _com = Com::start();
        let name = HSTRING::from(path);
        let info = OPENASINFO {
            pcszFile: PCWSTR(name.as_ptr()),
            pcszClass: PCWSTR::null(),
            oaifInFlags: OAIF_EXEC,
        };
        // SAFETY: the name outlives the dialog, which is modal.
        match unsafe { SHOpenWithDialog(Some(owner()), &info) } {
            Ok(()) => Ok(()),
            // ERROR_CANCELLED, as an HRESULT.
            Err(error) if error.code().0 as u32 == 0x8007_04C7 => Ok(()),
            Err(error) => Err(error).context("the system would not offer a program"),
        }
    }

    thread_local! {
        /// The menu being shown, for the hidden window to hand its messages
        /// to. One at a time: the menu is modal.
        static SHOWING: RefCell<(Option<IContextMenu3>, Option<IContextMenu2>)> =
            const { RefCell::new((None, None)) };
    }

    /// The hidden window's whole job: the messages the shell's own parts of
    /// the menu need go back to the shell.
    unsafe extern "system" fn host(
        hwnd: HWND,
        message: u32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
        if matches!(
            message,
            WM_INITMENUPOPUP | WM_DRAWITEM | WM_MEASUREITEM | WM_MENUCHAR
        ) {
            // Cloned out before the call: the shell may well send the window
            // another message while it is handling this one.
            let (three, two) = SHOWING.with(|showing| showing.borrow().clone());
            if let Some(three) = three {
                let mut result = LRESULT(0);
                // SAFETY: the message is passed through as it came.
                if unsafe { three.HandleMenuMsg2(message, wparam, lparam, Some(&mut result)) }
                    .is_ok()
                {
                    return result;
                }
            } else if let Some(two) = two {
                // SAFETY: as above.
                if unsafe { two.HandleMenuMsg(message, wparam, lparam) }.is_ok() {
                    return LRESULT(0);
                }
            }
        }

        // SAFETY: everything else is the default window's business.
        unsafe { DefWindowProcW(hwnd, message, wparam, lparam) }
    }

    /// The hidden window, for as long as the menu is up.
    struct Host(HWND);

    impl Host {
        fn new() -> Result<Self> {
            // SAFETY: registering a class twice fails harmlessly, and the
            // window is destroyed in Drop.
            unsafe {
                let instance = GetModuleHandleW(None)?;
                let class = w!("PhotoSiteShellMenu");
                RegisterClassW(&WNDCLASSW {
                    lpfnWndProc: Some(host),
                    hInstance: instance.into(),
                    lpszClassName: class,
                    ..Default::default()
                });
                let hwnd = CreateWindowExW(
                    WINDOW_EX_STYLE(0),
                    class,
                    w!(""),
                    WS_POPUP,
                    0,
                    0,
                    0,
                    0,
                    None,
                    None,
                    Some(instance.into()),
                    None,
                )?;
                Ok(Self(hwnd))
            }
        }
    }

    impl Drop for Host {
        fn drop(&mut self) {
            SHOWING.with(|showing| *showing.borrow_mut() = (None, None));
            // SAFETY: our own window, created in `new`.
            let _ = unsafe { DestroyWindow(self.0) };
        }
    }

    /// The menu Explorer would show for these photographs, where the
    /// pointer is. Shift held shows the extended one, as it does there.
    pub fn system_menu(paths: &[PathBuf]) -> Result<Chosen> {
        let _com = Com::start();
        let owner = owner();
        let menu: IContextMenu = ui_object(paths, owner)?;

        // SAFETY: plain Win32 calls on handles made and released here; the
        // menu interface is alive for all of them.
        unsafe {
            let shift = GetKeyState(i32::from(VK_SHIFT.0)) < 0;
            let control = GetKeyState(i32::from(VK_CONTROL.0)) < 0;
            let popup = CreatePopupMenu()?;
            let flags = if shift {
                CMF_NORMAL | CMF_EXTENDEDVERBS
            } else {
                CMF_NORMAL
            };
            if let Err(error) = menu.QueryContextMenu(popup, 0, FIRST, LAST, flags).ok() {
                let _ = DestroyMenu(popup);
                return Err(error).context("the system has no menu for these");
            }

            let host = Host::new()?;
            SHOWING.with(|showing| {
                *showing.borrow_mut() = (menu.cast().ok(), menu.cast().ok());
            });

            let mut at = POINT::default();
            let _ = GetCursorPos(&mut at);
            let _ = SetForegroundWindow(host.0);
            let picked = TrackPopupMenuEx(
                popup,
                TPM_RETURNCMD.0 | TPM_RIGHTBUTTON.0,
                at.x,
                at.y,
                host.0,
                None,
            )
            .0 as u32;
            // The menu's own advice: a message to the window after the menu,
            // or the next one it shows may not close properly.
            let _ = PostMessageW(Some(host.0), WM_NULL, WPARAM(0), LPARAM(0));
            drop(host);
            if !owner.is_invalid() {
                let _ = SetForegroundWindow(owner);
            }

            let outcome = if picked < FIRST {
                Ok(Chosen::Nothing)
            } else {
                run(&menu, picked - FIRST, owner, at, shift, control)
            };
            let _ = DestroyMenu(popup);
            outcome
        }
    }

    /// What the system's menu for these would say, without showing it.
    #[cfg(test)]
    pub fn system_menu_entries(paths: &[PathBuf]) -> Result<Vec<String>> {
        use windows::Win32::UI::WindowsAndMessaging::{
            GetMenuItemCount, GetMenuStringW, MF_BYPOSITION,
        };

        let _com = Com::start();
        let menu: IContextMenu = ui_object(paths, HWND::default())?;
        // SAFETY: a menu made, read and destroyed here.
        unsafe {
            let popup = CreatePopupMenu()?;
            let filled = menu
                .QueryContextMenu(popup, 0, FIRST, LAST, CMF_NORMAL)
                .ok();
            let mut entries = Vec::new();
            for at in 0..GetMenuItemCount(Some(popup)).max(0) {
                let mut text = [0u16; 256];
                let length = GetMenuStringW(popup, at as u32, Some(&mut text), MF_BYPOSITION);
                if length > 0 {
                    entries.push(String::from_utf16_lossy(&text[..length as usize]));
                }
            }
            let _ = DestroyMenu(popup);
            filled?;
            Ok(entries)
        }
    }

    /// Does what was picked — or hands it back, when it is something the
    /// application does itself.
    unsafe fn run(
        menu: &IContextMenu,
        offset: u32,
        owner: HWND,
        at: POINT,
        shift: bool,
        control: bool,
    ) -> Result<Chosen> {
        let mut verb = [0u16; 64];
        // SAFETY: the buffer is the size it says; the verb comes back wide
        // because GCS_VERBW asks for that.
        let named = unsafe {
            menu.GetCommandString(
                offset as usize,
                GCS_VERBW,
                None,
                PSTR(verb.as_mut_ptr().cast()),
                verb.len() as u32,
            )
        };
        if named.is_ok() {
            let end = verb
                .iter()
                .position(|&unit| unit == 0)
                .unwrap_or(verb.len());
            if let Some(command) = ours(&String::from_utf16_lossy(&verb[..end])) {
                return Ok(Chosen::Command(command));
            }
        }

        let mut mask = CMIC_MASK_UNICODE | CMIC_MASK_PTINVOKE;
        if shift {
            mask |= CMIC_MASK_SHIFT_DOWN;
        }
        if control {
            mask |= CMIC_MASK_CONTROL_DOWN;
        }

        // A command given by number is a pointer-sized integer in the verb
        // fields — MAKEINTRESOURCE, in the headers' words.
        let info = CMINVOKECOMMANDINFOEX {
            cbSize: std::mem::size_of::<CMINVOKECOMMANDINFOEX>() as u32,
            fMask: mask,
            hwnd: owner,
            lpVerb: PCSTR(offset as usize as *const u8),
            lpVerbW: PCWSTR(offset as usize as *const u16),
            nShow: SW_SHOWNORMAL.0,
            ptInvoke: at,
            ..Default::default()
        };
        // SAFETY: the structure is the extended one and says so in cbSize,
        // which is how the shell knows to read the rest of it.
        unsafe {
            menu.InvokeCommand(&info as *const CMINVOKECOMMANDINFOEX as *const CMINVOKECOMMANDINFO)
        }
        .context("the system could not do that")?;
        Ok(Chosen::Nothing)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_menu_speaks_for_one_folder() {
        let paths: Vec<PathBuf> = ["/a/one.jpg", "/a/b/two.jpg", "/a/three.jpg"]
            .iter()
            .map(PathBuf::from)
            .collect();
        assert_eq!(
            one_folder(&paths),
            [PathBuf::from("/a/one.jpg"), PathBuf::from("/a/three.jpg")]
        );
        assert!(one_folder(&[]).is_empty());
    }

    #[test]
    fn deleting_and_renaming_go_through_our_own_commands() {
        assert_eq!(ours("delete"), Some("file.delete"));
        assert_eq!(ours("Rename"), Some("file.rename"));
        assert_eq!(ours("properties"), None);
        assert_eq!(ours("print"), None);
    }

    #[test]
    fn the_system_menu_is_there_only_where_there_is_a_system_to_ask() {
        assert_eq!(AVAILABLE, cfg!(windows));
    }

    /// Asks this machine's shell, so it is run by hand: what opens a JPEG
    /// and what the menu offers depend on what is installed, and a runner
    /// with no desktop may have neither.
    #[cfg(windows)]
    #[test]
    #[ignore = "asks this machine's shell"]
    fn the_shell_knows_what_opens_a_jpeg_and_has_a_menu_for_one() {
        let dir = tempfile::tempdir().unwrap();
        let photos = [dir.path().join("a.jpg"), dir.path().join("b.jpg")];
        for path in &photos {
            std::fs::write(path, b"x").unwrap();
        }

        let programs = handlers("jpg");
        eprintln!("{programs:#?}");
        assert!(!programs.is_empty(), "nothing opens a JPEG");

        let entries = windows_shell::system_menu_entries(&photos).unwrap();
        eprintln!("{entries:#?}");
        assert!(entries.len() > 3, "{entries:?}");
    }
}
