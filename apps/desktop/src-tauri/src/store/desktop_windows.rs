//! Explorer's virtual desktop manager addresses a desktop by its persistent GUID.
use tauri::{Manager, Runtime, Window};
use uuid::Uuid;
use windows::{
    Win32::{
        System::Com::{CLSCTX_ALL, CoCreateInstance},
        UI::Shell::{IVirtualDesktopManager, VirtualDesktopManager},
    },
    core::GUID,
};
use winreg::{RegKey, enums::HKEY_CURRENT_USER};

pub(super) type Desktop = Uuid;

fn manager() -> Option<IVirtualDesktopManager> {
    // SAFETY: Tauri initializes COM on the UI thread. The CLSID implements this interface.
    unsafe { CoCreateInstance(&VirtualDesktopManager, None, CLSCTX_ALL).ok() }
}

pub(super) fn current<R: Runtime>(window: &Window<R>) -> Option<Desktop> {
    let hwnd = window.hwnd().ok()?;
    // SAFETY: the handle belongs to this live Tauri window.
    let id = unsafe { manager()?.GetWindowDesktopId(hwnd) }.ok()?;
    Some(Uuid::from_u128(id.to_u128())).filter(|id| !id.is_nil())
}

pub(super) fn place<R: Runtime>(window: &Window<R>, desktop: &Desktop) {
    let (Ok(hwnd), Some(manager)) = (window.hwnd(), manager()) else {
        return;
    };
    // SAFETY: the handle is our own live window and both GUIDs live through their calls.
    unsafe {
        if manager
            .MoveWindowToDesktop(hwnd, &GUID::from_u128(desktop.as_u128()))
            .is_err()
            && let Some(first) = first_desktop().or_else(|| {
                let main = window.get_webview_window(crate::store::window_label::LABEL)?;
                manager.GetWindowDesktopId(main.hwnd().ok()?).ok()
            })
            && let Err(error) = manager.MoveWindowToDesktop(hwnd, &first)
        {
            log::debug!("placing a window on the first desktop: {error}");
        }
    }
}

fn first_desktop() -> Option<GUID> {
    let key = RegKey::predef(HKEY_CURRENT_USER)
        .open_subkey(r"Software\Microsoft\Windows\CurrentVersion\Explorer\VirtualDesktops")
        .ok()?;
    let list = key.get_raw_value("VirtualDesktopIDs").ok()?;
    let first = Uuid::from_bytes_le(list.bytes.get(..16)?.try_into().ok()?);
    Some(GUID::from_u128(first.as_u128()))
}
