//! Reading a child's output a line at a time, dropping a line over the cap
//! (`LineReader`).

use std::io;
use tokio::io::{AsyncBufRead, AsyncBufReadExt};

/// The longest message read from an agent; a longer one is dropped.
pub(crate) const MAX_MESSAGE: usize = 16 * 1024 * 1024;

/// One line read by a `LineReader`.
#[derive(Debug, PartialEq)]
pub(crate) enum Line {
    /// The line without its newline.
    Text(String),
    /// A line longer than the cap, dropped; its length in bytes.
    TooLong(usize),
}

/// Reads lines without holding more than `max` bytes of any one: the rest
/// of a longer line is read and thrown away.
pub(crate) struct LineReader<R> {
    reader: R,
    max: usize,
    line: Vec<u8>,
}

impl<R: AsyncBufRead + Unpin> LineReader<R> {
    pub(crate) fn new(reader: R, max: usize) -> Self {
        Self {
            reader,
            max,
            line: Vec::new(),
        }
    }

    /// The next line; `None` at the end of the input.
    pub(crate) async fn next(&mut self) -> io::Result<Option<Line>> {
        self.line.clear();
        let mut length = 0usize;

        loop {
            let available = self.reader.fill_buf().await?;

            if available.is_empty() {
                // The end of the input ends a last line that has no newline.
                return Ok((length > 0).then(|| self.finish(length)));
            }

            let (part, used, ended) = match available.iter().position(|&b| b == b'\n') {
                Some(at) => (&available[..at], at + 1, true),
                None => (available, available.len(), false),
            };
            length = length.saturating_add(part.len());
            if length <= self.max {
                self.line.extend_from_slice(part);
            } else {
                // Nothing of an oversized line is kept.
                self.line = Vec::new();
            }
            self.reader.consume(used);

            if ended {
                return Ok(Some(self.finish(length)));
            }
        }
    }

    fn finish(&mut self, length: usize) -> Line {
        if length > self.max {
            return Line::TooLong(length);
        }

        let mut text = String::from_utf8_lossy(&self.line).into_owned();

        if text.ends_with('\r') {
            text.pop();
        }

        Line::Text(text)
    }
}

#[cfg(test)]
#[path = "../tests/process_line_reader.rs"]
mod tests;
