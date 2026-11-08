//! The user's saved agent instructions and their versions.

use super::Reply;
use crate::{
    api::router::Cx,
    protocol::*,
    store::{InstructionsRow, Saved},
};

/// The longest instructions, in bytes.
const MAX_TEXT: usize = 16 * 1024;
const MAX_NOTE: usize = 200;

pub(super) async fn get_instructions(
    cx: Cx,
    _: GetInstructions,
) -> Reply<Option<InstructionsVersion>> {
    Ok(newest(&cx).await?.map(view))
}

pub(super) async fn save_instructions(
    cx: Cx,
    request: SaveInstructions,
) -> Reply<InstructionsVersion> {
    if request.text.len() > MAX_TEXT {
        return Err(ApiError::bad_request(
            "Instructions can be at most 16 KB.".to_owned(),
        ));
    }
    let note = request.note.trim();
    if note.len() > MAX_NOTE || note.chars().any(char::is_control) {
        return Err(ApiError::bad_request(format!(
            "A note can be at most {MAX_NOTE} characters on one line."
        )));
    }
    match cx
        .user
        .save_instructions(request.base, &request.text, note)
        .await?
    {
        Saved::Made(row) => Ok(view(row)),
        Saved::Stale(newest) => Err(ApiError::Conflict {
            message: format!("Version {newest} was saved since. Load it, then edit again."),
        }),
    }
}

pub(super) async fn list_instructions_versions(
    cx: Cx,
    _: ListInstructionsVersions,
) -> Reply<Vec<InstructionsVersion>> {
    Ok(cx
        .user
        .instructions_versions(50)
        .await?
        .into_iter()
        .map(view)
        .collect())
}

async fn newest(cx: &Cx) -> Result<Option<InstructionsRow>, ApiError> {
    Ok(cx.user.instructions_versions(1).await?.into_iter().next())
}

fn view(row: InstructionsRow) -> InstructionsVersion {
    InstructionsVersion {
        version: row.version,
        text: row.text,
        note: row.note,
        saved_at: row.saved_at.into(),
    }
}
