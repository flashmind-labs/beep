//! Voice notes: whether a notice is read aloud, and reading one out in
//! live mode, which only plays and never listens.

use super::{
    Voice,
    presence::Presence,
    reader::{Ready, Voiced},
    session::Start,
};
use crate::{bar, beep::state::NoticeVoice};
use beep_live::Input;
use beep_proto::{Notice, SpeakNotice, VoiceNote};
use std::sync::Arc;
use tokio::sync::{mpsc, watch};

impl Voice {
    /// What `notice` does: offered on the orb's card or spoken when the
    /// Voice notes choice says so, the bar is closed, live mode is off, and
    /// the user is here and not on a call. Otherwise it is only a banner.
    pub fn wants(&self, notice: &Notice) -> NoticeVoice {
        let choice = self.beep.notice_voice();
        let fits = choice != NoticeVoice::Off
            && notice.approval.is_none()
            && notice.app.is_none()
            && !(notice.title.trim().is_empty() && notice.body.trim().is_empty())
            && !bar::is_open(&self.app)
            && !self.is_live()
            && Presence::now().can_speak();
        if fits { choice } else { NoticeVoice::Off }
    }

    /// Reads `notice` out in live mode, or shows its banner when that fails.
    pub async fn speak_notice(self: Arc<Self>, notice: Notice) {
        let note = self
            .beep
            .call(SpeakNotice {
                title: notice.title.clone(),
                body: notice.body.clone(),
            })
            .await;
        let note = match note {
            Ok(note) => note,
            Err(e) => {
                log::warn!("speaking a notice: {e}");
                crate::notices::notify::notice(&self.app, &notice);
                return;
            }
        };
        if self.is_live() || bar::is_open(&self.app) {
            crate::notices::notify::notice(&self.app, &notice);
            return;
        }
        let title = notice.title.clone();
        let notice = Box::new(notice);
        self.start(
            Start::Note {
                note,
                title,
                notice,
            },
            // Only played, so it stays out of the open chat.
            None,
        );
    }

    /// Plays the voice note that started live mode.
    pub(super) fn read_note(self: &Arc<Self>, id: u64, note: VoiceNote) {
        let me = self.clone();
        let reader = async move {
            let ready = Ready {
                text: note.text,
                audio: note.audio.data,
                filler: false,
            };
            let (tx, rx) = mpsc::channel(1);
            // The channel has room for it.
            let _ = tx.try_send(ready);
            drop(tx);
            let voiced = Voiced::new(1);
            let (_, ticks) = watch::channel(0);
            me.play(id, rx, ticks, &voiced, false).await;
            // Read: it ends here rather than listening for an answer.
            me.input(id, Input::Escape);
        };
        if let Some(s) = self.state().live.as_mut().filter(|s| s.id == id) {
            s.reader.start(reader);
        }
    }
}
