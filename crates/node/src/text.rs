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

/// The longest session id accepted.
const MAX_ID: usize = 128;

/// An id that cannot name anything outside a store: letters, digits, `-`
/// and `_`, so no separator, no dot and no `..`.
pub(crate) fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= MAX_ID
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}
