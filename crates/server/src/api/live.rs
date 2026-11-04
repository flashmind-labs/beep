//! Live mode's calls besides `Transcribe`.

use super::Reply;
use crate::{
    api::router::Cx,
    protocol::{ApiError, Filler, MAX_SAY_CHARS, Say, SpokenTurns, VoiceNote},
    voice::filler,
};

/// Longest step label or message a `Filler` reads.
const MAX_FILLER_INPUT: usize = 4096;

pub(super) async fn spoken_turns(
    cx: Cx,
    SpokenTurns { session_id }: SpokenTurns,
) -> Reply<Vec<i64>> {
    Ok(cx.user.spoken_turns(session_id).await?)
}

pub(super) async fn say(cx: Cx, Say { text }: Say) -> Reply<VoiceNote> {
    if text.chars().count() > MAX_SAY_CHARS {
        return Err(ApiError::bad_request(format!(
            "at most {MAX_SAY_CHARS} characters at a time"
        )));
    }
    cx.app.say(&cx.user, &text).await
}

pub(super) async fn filler(cx: Cx, Filler { step, said, still }: Filler) -> Reply<VoiceNote> {
    if step.len() > MAX_FILLER_INPUT || said.len() > MAX_FILLER_INPUT {
        return Err(ApiError::bad_request("too long"));
    }
    cx.app
        .say(&cx.user, &filler::line(&step, &said, still))
        .await
}
