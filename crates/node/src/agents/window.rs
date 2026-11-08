//! Which entries of a transcript a read answers with.

use std::ops::Range;

/// The positions, among `len` entries whose first has the index `first`,
/// of the `last` entries before the entry with index `before` (the end of
/// the transcript when `None`). Positions before the first entry or past
/// the end are cut off, so a `before` the node no longer reaches gives an
/// empty range.
pub(super) fn window(first: u64, len: usize, last: usize, before: Option<u64>) -> Range<usize> {
    let end = before
        .map_or(len, |at| {
            usize::try_from(at.saturating_sub(first)).unwrap_or(usize::MAX)
        })
        .min(len);
    end.saturating_sub(last)..end
}

#[cfg(test)]
#[path = "../tests/agents_window.rs"]
mod tests;
