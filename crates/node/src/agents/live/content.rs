//! Reading what the agent sends: pictures, cut text and thoughts.

use super::MAX_LINE;
use crate::agents::images::{Picture, data_url};
use agent_client_protocol::schema::v1::{ImageContent, ToolCallId};
use base64::{Engine, engine::general_purpose::STANDARD};
use serde_json::Value;

/// The picture of an ACP image block.
pub(super) fn image_picture(image: &ImageContent) -> Option<Picture> {
    picture_of(&image.mime_type, &image.data)
}

/// The picture `data`, in base64, holds when `mime` is an image type.
pub(super) fn picture_of(mime: &str, data: &str) -> Option<Picture> {
    if !mime.starts_with("image/") {
        return None;
    }
    let data = STANDARD.decode(data.trim()).ok()?;
    Some(Picture {
        mime: mime.to_owned(),
        data,
    })
}

/// What a permission question would act on: the command, the file, or the
/// input itself.
pub(super) fn detail(input: &Value) -> Option<String> {
    let text = ["command", "file_path", "path", "url"]
        .iter()
        .find_map(|key| input[key].as_str())
        .map(str::to_owned)
        .or_else(|| (!input.is_null()).then(|| input.to_string()))?;
    Some(cut(&text, MAX_LINE))
}

/// `text` cut to at most `max` bytes, on a character boundary.
pub(crate) fn cut(text: &str, max: usize) -> String {
    let mut out = String::new();
    push_cut(&mut out, text, max);
    out
}

pub(super) fn push_cut(out: &mut String, text: &str, max: usize) {
    let room = max.saturating_sub(out.len());
    if text.len() <= room {
        out.push_str(text);
        return;
    }
    let mut end = room;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    out.push_str(&text[..end]);
}

/// `text` as an agent replays it, with each image it inlined as a markdown
/// link to a data URL, `[@name](data:image/png;base64,...)`, made an image
/// line of its own. The pictures go in `found`.
pub(super) fn replayed(text: &str, found: &mut Vec<Picture>) -> String {
    let mut out = String::new();
    let mut rest = text;
    while let Some(at) = rest.find("](data:image/") {
        let url_at = at + 2;
        let (Some(open), Some(len)) = (rest[..at].rfind('['), rest[url_at..].find(')')) else {
            break;
        };
        let picture = data_url(&rest[url_at..url_at + len]);
        let end = url_at + len + 1;
        match picture {
            Some(picture) => {
                out.push_str(rest[..open].trim_end_matches(' '));
                if !out.is_empty() && !out.ends_with('\n') {
                    out.push('\n');
                }
                out.push_str(&picture.line());
                found.push(picture);
                if !rest[end..].is_empty() && !rest[end..].starts_with('\n') {
                    out.push('\n');
                }
            }
            None => out.push_str(&rest[..end]),
        }
        rest = &rest[end..];
    }
    out.push_str(rest);
    out
}

/// The subagent tool call a call ran in, as Claude Code's adapter stamps
/// it (`_meta.claudeCode.parentToolUseId`).
pub(super) fn parent_tool(meta: Option<&serde_json::Map<String, Value>>) -> Option<ToolCallId> {
    let id = meta?.get("claudeCode")?.get("parentToolUseId")?.as_str()?;
    Some(ToolCallId::new(id.to_owned()))
}

/// The line a thought shows while it arrives: its last bold heading
/// (`**Planning the fix**`), else its first sentence once that is whole.
/// `None` until there is one.
pub(super) fn thought_line(text: &str) -> Option<String> {
    let mut heading = None;
    let mut rest = text;
    while let Some(start) = rest.find("**") {
        let after = &rest[start + 2..];
        let Some(end) = after.find("**") else {
            break;
        };
        let bold = after[..end].trim();
        if !bold.is_empty() && !bold.contains('\n') {
            heading = Some(bold);
        }
        rest = &after[end + 2..];
    }
    if let Some(heading) = heading {
        return Some(heading.to_owned());
    }
    let text = text.trim_start();
    let mut chars = text.char_indices().peekable();
    while let Some((at, c)) = chars.next() {
        let end = match c {
            '\n' => Some(at),
            '.' | '!' | '?' if chars.peek().is_some_and(|(_, next)| next.is_whitespace()) => {
                Some(at + c.len_utf8())
            }
            _ => None,
        };
        if let Some(end) = end {
            let line: String = text[..end]
                .chars()
                .filter(|c| !matches!(c, '*' | '`'))
                .collect();
            let line = line.trim();
            return (!line.is_empty()).then(|| line.to_owned());
        }
    }
    None
}
