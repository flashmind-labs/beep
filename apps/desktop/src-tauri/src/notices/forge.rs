//! Review requests and assignments on git hosts as notes and notifications: what the reads of
//! `projects::git_items::forge_source` say began to ask for the user.

use crate::{
    beep::state::Prefs,
    notes::{self, Note, NoteKind},
    notices::show_new,
    projects::forge_notices::{Flag, Found, Marked, Seen},
    targets::CenterTarget,
};
use std::time::Instant;
use tauri::{AppHandle, Manager};
use tokio::sync::mpsc;

/// Follows each read of the git hosts for as long as the app runs.
pub(crate) async fn follow_reads(app: AppHandle, mut reads: mpsc::UnboundedReceiver<Found>) {
    let mut seen = Seen::default();

    while let Some(found) = reads.recv().await {
        follow(&app, &mut seen, &found);
    }
}

/// The note for a pull request or issue that began to ask for the user, as it was updated at
/// `updated_at` (milliseconds since the epoch). The id is the same on every app, and a later
/// request is another note.
fn note(flag: Flag, item: &Marked) -> Note {
    let (kind, id, title) = match flag {
        Flag::Review => (NoteKind::ReviewRequested, "review", "Review requested"),
        Flag::Assigned => (NoteKind::Assigned, "assigned", "Assigned"),
    };
    Note {
        text: format!("{}#{}", item.repo, item.number),
        target: Some(CenterTarget::Link(item.url.clone())),
        at: notes::now_ms(),
        ..Note::new(
            kind,
            format!("{id}:{}:{}", item.url, item.updated_at),
            format!("{title}: {}", item.title),
            item.repo.clone(),
        )
    }
}

/// A pull request or issue began to ask for the user: the note it makes, `None` when one was
/// made already (another app told it too). The note is read when another app read it.
fn raised(app: &AppHandle, flag: Flag, item: &Marked) -> Option<Note> {
    notes::recorded(app, note(flag, item))
}

/// What asked for the user on the item at `url` ended, or the item is gone.
fn cleared(app: &AppHandle, flag: Flag, url: &str) {
    let prefix = match flag {
        Flag::Review => format!("review:{url}:"),
        Flag::Assigned => format!("assigned:{url}:"),
    };
    notes::finish(app, |note| note.id.starts_with(&prefix));
}

/// Reads `items` against the last read: a new request or assignment is a note and a
/// notification when they are on, one that ended is done. Off, it
/// makes none but still keeps its baseline and finishes what ended.
fn follow(app: &AppHandle, seen: &mut Seen, found: &Found) {
    let changes = seen.update(&found.items, &found.answered);

    for (flag, url) in changes.cleared {
        cleared(app, flag, &url);
    }

    if !app.state::<Prefs>().forge_notices() {
        return;
    }

    for (flag, item) in changes.raised {
        if let Some(note) = raised(app, flag, item) {
            let body = note.text.clone();
            show_new(app, note, body, Instant::now());
        }
    }
}
