//! Spots a model stuck emitting the same text over and over.

/// How many back-to-back copies of a chunk count as a loop.
const COPIES: usize = 4;
/// Shorter chunks are not checked on their own; a short chunk repeated long
/// enough still shows up as a longer one.
const MIN_PERIOD: usize = 24;
const MAX_PERIOD: usize = 600;
/// Only re-check after this many new characters, so the scan stays cheap.
const CHECK_EVERY: usize = 128;

#[derive(Default)]
pub struct RepeatGuard {
    tail: Vec<char>,
    unchecked: usize,
}

impl RepeatGuard {
    /// Adds streamed text. Returns true once the tail of everything seen is
    /// one chunk repeated [`COPIES`] times.
    pub fn push(&mut self, delta: &str) -> bool {
        let before = self.tail.len();
        self.tail.extend(delta.chars());
        self.unchecked += self.tail.len() - before;
        let keep = MAX_PERIOD * COPIES;
        if self.tail.len() > keep {
            self.tail.drain(..self.tail.len() - keep);
        }
        if self.unchecked < CHECK_EVERY {
            return false;
        }
        self.unchecked = 0;
        (MIN_PERIOD..=MAX_PERIOD)
            .take_while(|period| period * COPIES <= self.tail.len())
            .any(|period| self.repeats(period))
    }

    fn repeats(&self, period: usize) -> bool {
        let tail = &self.tail[self.tail.len() - period * COPIES..];
        let (first, rest) = tail.split_at(period);
        // Whitespace alone repeating is not a loop worth stopping for.
        !first.iter().all(|c| c.is_whitespace()) && rest.chunks(period).all(|chunk| chunk == first)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stops_a_repeated_sentence() {
        let mut guard = RepeatGuard::default();
        let looped = (0..20).any(|_| guard.push("I will check the inbox now. "));
        assert!(looped);
    }

    #[test]
    fn leaves_normal_text_alone() {
        let mut guard = RepeatGuard::default();
        for i in 0..200 {
            assert!(!guard.push(&format!("Line {i} says something different. ")));
        }
        let mut guard = RepeatGuard::default();
        assert!(!guard.push("| a | b |\n| --- | --- |\n| 1 | 2 |\n| 3 | 4 |\n"));
        let mut guard = RepeatGuard::default();
        assert!(!guard.push(&" ".repeat(5000)));
    }
}
