//! EWMH desktops on X11. Native Wayland toplevels are placed by the compositor.
use gdkx11::{X11Screen, X11Window, gdk};
use gtk::prelude::*;
use serde::{Deserialize, Serialize};
use tauri::{Runtime, Window};

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct Desktop {
    index: u32,
    name: Option<String>,
}

pub(super) fn current<R: Runtime>(window: &Window<R>) -> Option<Desktop> {
    let (native, screen) = x11(window)?;
    let index = native.desktop();
    let name = names(&screen)
        .get(usize::try_from(index).ok()?)
        .filter(|name| !name.is_empty())
        .cloned();
    Some(Desktop { index, name })
}

pub(super) fn place<R: Runtime>(window: &Window<R>, desk: &Desktop) {
    if let Some((native, screen)) = x11(window) {
        native.move_to_desktop(desk.destination(screen.number_of_desktops(), &names(&screen)));
    }
}

impl Desktop {
    fn destination(&self, count: u32, names: &[String]) -> u32 {
        // EWMH reserves this value for a window on every desktop.
        if self.index == u32::MAX {
            return self.index;
        }
        if let Some(name) = &self.name
            && !names.is_empty()
        {
            if names.get(self.index as usize) == Some(name) && self.index < count {
                return self.index;
            }
            return names
                .iter()
                .take(count as usize)
                .position(|n| n == name)
                .and_then(|i| u32::try_from(i).ok())
                .unwrap_or(0);
        }
        if self.index < count { self.index } else { 0 }
    }
}

fn x11<R: Runtime>(window: &Window<R>) -> Option<(X11Window, X11Screen)> {
    let native = window.gtk_window().ok()?.window()?;
    let screen = native.screen().downcast::<X11Screen>().ok()?;
    if !screen.supports_net_wm_hint(&gdk::Atom::intern("_NET_WM_DESKTOP")) {
        return None;
    }

    Some((native.downcast().ok()?, screen))
}

fn names(screen: &X11Screen) -> Vec<String> {
    let Some(root) = screen.upcast_ref::<gdk::Screen>().root_window() else {
        return Vec::new();
    };
    let Some((_, 8, bytes)) = gdk::property_get(
        &root,
        &gdk::Atom::intern("_NET_DESKTOP_NAMES"),
        &gdk::Atom::intern("UTF8_STRING"),
        0,
        65536,
        0,
    ) else {
        return Vec::new();
    };
    String::from_utf8_lossy(&bytes)
        .split_terminator('\0')
        .map(str::to_owned)
        .collect()
}

#[cfg(test)]
#[path = "../tests/desktop_x11.rs"]
mod tests;
