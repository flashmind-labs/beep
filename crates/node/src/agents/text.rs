//! Text helpers shared by the agents modules.

/// `text` cut to at most `max` bytes, on a character boundary.
pub(crate) fn cut(text: &str, max: usize) -> String {
    let mut out = String::new();
    push_cut(&mut out, text, max);
    out
}

pub(crate) fn push_cut(out: &mut String, text: &str, max: usize) {
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
