//! Text helpers shared by the agents modules.

/// `text` cut to at most `max` bytes, on a character boundary.
pub(crate) fn cut(text: &str, max: usize) -> String {
    let mut out = String::new();
    push_cut(&mut out, text, max);
    out
}

pub(crate) fn push_cut(out: &mut String, text: &str, max: usize) {
    out.push_str(cut_str(text, max.saturating_sub(out.len())));
}

/// The start of `text` up to `max` bytes, on a character boundary.
pub(crate) fn cut_str(text: &str, max: usize) -> &str {
    &text[..text.floor_char_boundary(max)]
}
