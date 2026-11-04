//! A terminal's kept output: its last lines, and how it stands.

use beep_proto::node::{
    process::TaskState,
    terminal::{MAX_OUTPUT, MAX_SCROLLBACK},
};
use std::collections::VecDeque;

/// The longest line the scrollback keeps whole, in bytes; a longer one is
/// kept as several.
const MAX_LINE: usize = 1024;

/// The kept output and how the terminal stands.
pub(super) struct Screen {
    /// The last lines, each ending with its newline but the last, which
    /// may still grow.
    pub(super) lines: VecDeque<Vec<u8>>,
    /// The offset of the first kept byte in all the output so far.
    start: u64,
    /// The bytes in `lines`.
    len: usize,
    pub(super) state: TaskState,
}

impl Screen {
    pub(super) fn new() -> Self {
        Self {
            lines: VecDeque::new(),
            start: 0,
            len: 0,
            state: TaskState::Running,
        }
    }

    pub(super) fn end(&self) -> u64 {
        self.start + self.len as u64
    }

    pub(super) fn push(&mut self, mut data: &[u8]) {
        while !data.is_empty() {
            let open = self
                .lines
                .back()
                .is_some_and(|line| line.len() < MAX_LINE && line.last() != Some(&b'\n'));
            if !open {
                self.lines.push_back(Vec::new());
            }
            let Some(line) = self.lines.back_mut() else {
                return;
            };
            let room = MAX_LINE - line.len();
            let take = data
                .iter()
                .take(room)
                .position(|&byte| byte == b'\n')
                .map_or(room.min(data.len()), |newline| newline + 1);
            line.extend_from_slice(&data[..take]);
            self.len += take;
            data = &data[take..];
        }
        while self.lines.len() > MAX_SCROLLBACK {
            if let Some(line) = self.lines.pop_front() {
                self.start += line.len() as u64;
                self.len -= line.len();
            }
        }
    }

    /// Up to `MAX_OUTPUT` bytes from `from`, or from the oldest kept when
    /// `from` is no longer kept, with where they start.
    pub(super) fn read_from(&self, from: u64) -> Option<(u64, Vec<u8>)> {
        let at = from.clamp(self.start, self.end());
        let mut skip = usize::try_from(at - self.start).ok()?;
        let mut data = Vec::new();
        for line in &self.lines {
            if skip >= line.len() {
                skip -= line.len();
                continue;
            }
            let take = (line.len() - skip).min(MAX_OUTPUT - data.len());
            data.extend_from_slice(&line[skip..skip + take]);
            skip = 0;
            if data.len() == MAX_OUTPUT {
                break;
            }
        }
        (!data.is_empty()).then_some((at, data))
    }
}
