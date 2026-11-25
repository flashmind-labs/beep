//! Detached windows to reopen, and the desktop to use next time even after
//! an explicit close. The window-state plugin owns each label's geometry.
use super::{lock, write_part};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, fs, io::ErrorKind, path::PathBuf, sync::Mutex};
use tauri::{AppHandle, Manager, Runtime, Window};
use tauri_plugin_window_state::AppHandleExt;

/// Labels of the windows that each show one project, or one chat.
const PROJECT_PREFIX: &str = "project-";
const CHAT_PREFIX: &str = "chat-";

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
#[path = "desktop_x11.rs"]
mod desktop;
#[cfg(target_os = "macos")]
#[path = "desktop_macos.rs"]
mod desktop;
#[cfg(target_os = "windows")]
#[path = "desktop_windows.rs"]
mod desktop;

/// What a window apart from the main one shows alone: a project, or an
/// assistant chat with its threads. Its label says which.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Alone {
    Project { project: String },
    Chat { machine_id: String, session: String },
}

impl Alone {
    /// The window's label: a prefix, then the name in hex, since a label
    /// takes only letters, digits and a few signs.
    pub(super) fn label(&self) -> String {
        match self {
            Self::Project { project } => format!("{PROJECT_PREFIX}{}", hex(project)),
            Self::Chat {
                machine_id,
                session,
            } => format!("{CHAT_PREFIX}{}", hex(&format!("{machine_id}\n{session}"))),
        }
    }

    /// What window `label` shows, `None` for the main one and any other.
    pub(super) fn of(label: &str) -> Option<Self> {
        if let Some(name) = label.strip_prefix(PROJECT_PREFIX) {
            return Some(Self::Project {
                project: unhex(name)?,
            });
        }

        let chat = unhex(label.strip_prefix(CHAT_PREFIX)?)?;
        let (machine_id, session) = chat.split_once('\n')?;
        Some(Self::Chat {
            machine_id: machine_id.to_owned(),
            session: session.to_owned(),
        })
    }

    /// The window's title until its page names it.
    pub(super) fn title(&self) -> &str {
        match self {
            Self::Project { project } => project,
            Self::Chat { .. } => "Assistant",
        }
    }
}

/// Whether `label` is a window showing a project or a chat alone.
pub(super) fn alone(label: &str) -> bool {
    label.starts_with(PROJECT_PREFIX) || label.starts_with(CHAT_PREFIX)
}

pub(super) fn hex(text: &str) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut hex = String::with_capacity(text.len() * 2);

    for byte in text.bytes() {
        hex.push(char::from(DIGITS[usize::from(byte >> 4)]));
        hex.push(char::from(DIGITS[usize::from(byte & 0xf)]));
    }

    hex
}

fn unhex(hex: &str) -> Option<String> {
    if hex.is_empty() || !hex.len().is_multiple_of(2) {
        return None;
    }

    let bytes = (0..hex.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(hex.get(i..i + 2)?, 16).ok())
        .collect::<Option<Vec<u8>>>()?;
    String::from_utf8(bytes).ok()
}

#[derive(Default, Serialize, Deserialize)]
struct Saved {
    open: bool,
    desktop: Option<desktop::Desktop>,
}

#[derive(Default)]
struct Session {
    windows: BTreeMap<String, Saved>,
    quitting: bool,
    pending: bool,
}

pub(crate) struct WindowSession {
    path: PathBuf,
    session: Mutex<Session>,
}

impl WindowSession {
    pub(crate) fn load(path: PathBuf) -> Self {
        let result = fs::read(&path).and_then(|bytes| Ok(serde_json::from_slice(&bytes)?));
        let windows = result.unwrap_or_else(|error| {
            if error.kind() != ErrorKind::NotFound {
                log::warn!("reading saved windows: {error}");
            }
            BTreeMap::new()
        });

        Self {
            path,
            session: Mutex::new(Session {
                windows,
                ..Session::default()
            }),
        }
    }

    pub(crate) fn open_windows(&self) -> Vec<Alone> {
        lock(&self.session)
            .windows
            .iter()
            .filter(|(_, saved)| saved.open)
            .filter_map(|(label, _)| Alone::of(label))
            .collect()
    }

    fn save(&self, session: &Session) {
        if let Err(error) = serde_json::to_vec(&session.windows)
            .map_err(anyhow::Error::from)
            .and_then(|bytes| write_part(&self.path, &bytes))
        {
            log::warn!("saving detached windows: {error}");
        }
    }

    fn record(&self, label: &str, open: bool, desk: Option<desktop::Desktop>) {
        let mut session = lock(&self.session);
        if session.quitting {
            return;
        }

        let saved = session.windows.entry(label.to_owned()).or_default();
        saved.open = open;
        if desk.is_some() {
            saved.desktop = desk;
        }
        self.save(&session);
    }
}

/// Runs on the window thread, where native desktop APIs can access the window.
pub(super) fn opened<R: Runtime>(window: &Window<R>) {
    let store = window.state::<WindowSession>();
    let desk = lock(&store.session)
        .windows
        .get(window.label())
        .and_then(|s| s.desktop.as_ref())
        .cloned();
    if let Some(desk) = desk {
        desktop::place(window, &desk);
        store.record(window.label(), true, None);
    } else {
        store.record(window.label(), true, desktop::current(window));
    }
}

pub(crate) fn closing<R: Runtime>(window: &Window<R>) {
    if alone(window.label()) {
        window
            .state::<WindowSession>()
            .record(window.label(), false, desktop::current(window));
    }
}

/// Coalesce geometry and focus changes. A system shutdown may stop the process
/// without going through the tray or the updater.
pub(super) fn changed<R: Runtime>(window: &Window<R>) {
    if !alone(window.label()) {
        return;
    }
    let app = window.app_handle();
    let Some(store) = app.try_state::<WindowSession>() else {
        return;
    };
    {
        let mut session = lock(&store.session);
        if session.pending || session.quitting {
            return;
        }
        session.pending = true;
    }

    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
        let handle = app.clone();
        let _ = app.run_on_main_thread(move || {
            let store = handle.state::<WindowSession>();
            {
                let mut session = lock(&store.session);
                session.pending = false;
                if session.quitting {
                    return;
                }
            }
            save_open(&handle, &store);
        });
    });
}

/// Capture desktop assignments before the updater or normal quit hides the windows.
pub(crate) fn checkpoint(app: &AppHandle) {
    let Some(store) = app.try_state::<WindowSession>() else {
        return;
    };
    {
        let mut session = lock(&store.session);
        if session.quitting {
            return;
        }
        session.quitting = true;
    }
    save_open(app, &store);
}

fn save_open<R: Runtime>(app: &AppHandle<R>, store: &WindowSession) {
    let windows = app.webview_windows();
    let desktops = windows
        .values()
        .filter(|w| alone(w.label()))
        .filter_map(|w| desktop::current(&w.as_ref().window()).map(|d| (w.label(), d)))
        .collect::<Vec<_>>();
    {
        let mut session = lock(&store.session);
        let mut changed = false;
        for (label, desktop) in desktops {
            if let Some(saved) = session.windows.get_mut(label)
                && saved.open
                && saved.desktop.as_ref() != Some(&desktop)
            {
                saved.desktop = Some(desktop);
                changed = true;
            }
        }
        if changed {
            store.save(&session);
        }
    }

    if let Err(error) = app.save_window_state(crate::WINDOW_GEOMETRY) {
        log::warn!("saving window positions: {error}");
    }
}

#[cfg(test)]
#[path = "../tests/window_session.rs"]
mod tests;
