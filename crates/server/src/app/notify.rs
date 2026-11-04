//! Notices for finished scheduled runs.

use super::{App, Reach, UserNotice};
use crate::{
    account::user::User,
    protocol::{Notice, Run},
    store::RunStatus,
};

impl App {
    /// Tells the phone a scheduled run finished, showing its reply when it completed.
    pub async fn notify_run_complete(
        &self,
        user: &User,
        run: &Run,
        status: RunStatus,
        reply: &str,
    ) {
        // Titled after the session, or "Beep" when it cannot be found.
        let title = match user.sessions().await {
            Ok(sessions) => sessions
                .into_iter()
                .find(|s| s.id == run.session_id)
                .map(|s| s.title)
                .unwrap_or_else(|| "Beep".to_owned()),
            Err(_) => "Beep".to_owned(),
        };
        let reply = notification_text(reply);
        let body = match status {
            RunStatus::Completed if !reply.is_empty() => reply.as_str(),
            RunStatus::Completed => "Your request completed successfully.",
            RunStatus::Cancelled => "The run was cancelled.",
            RunStatus::Interrupted => "The server restarted before the run finished.",
            RunStatus::TimedOut => "The run timed out.",
            RunStatus::Stalled => "The model stopped responding.",
            RunStatus::Looped => "The model got stuck repeating itself and was stopped.",
            RunStatus::Failed => "The run failed.",
            RunStatus::Waiting => "Waiting for your OK.",
        }
        .to_owned();
        let notice = UserNotice {
            user: user.id().to_owned(),
            notice: Notice {
                title,
                body,
                session_id: run.session_id.clone(),
                app: None,
                approval: None,
                changed: false,
                group: false,
            },
            desktop: None,
            to: Reach::Everyone,
        };
        // Best-effort: ignore send errors (no active receivers).
        let _ = self.notifications.send(notice);
    }
}

/// Longest notification body, in characters; the full reply stays in the chat.
const NOTICE_CHARS: usize = 400;

/// A reply as notification text: emphasis and code marks dropped, cut to `NOTICE_CHARS`.
fn notification_text(reply: &str) -> String {
    let plain = reply.replace("**", "").replace("__", "").replace('`', "");
    let plain = plain.trim();
    match plain.char_indices().nth(NOTICE_CHARS) {
        Some((end, _)) => format!("{}...", plain[..end].trim_end()),
        None => plain.to_owned(),
    }
}
