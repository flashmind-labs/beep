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
mod tests {
    use super::*;

    #[test]
    fn pages_walk_back_without_a_gap_or_a_repeat() {
        let (first, len) = (7, 25);
        let mut seen = Vec::new();
        let mut before = None;
        loop {
            let range = window(first, len, 10, before);
            if range.is_empty() {
                break;
            }
            seen.splice(0..0, range.clone());
            before = Some(first + range.start as u64);
        }
        assert_eq!(seen, (0..len).collect::<Vec<_>>());
    }

    #[test]
    fn a_before_outside_what_is_kept_gives_nothing_or_the_tail() {
        assert_eq!(window(100, 50, 10, Some(90)), 0..0);
        assert_eq!(window(100, 50, 10, Some(100)), 0..0);
        assert_eq!(window(100, 50, 10, Some(500)), 40..50);
        assert_eq!(window(0, 5, 10, None), 0..5);
    }
}
