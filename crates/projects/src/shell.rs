//! Where a project's commands run.

use anyhow::Result;
use async_trait::async_trait;
use std::time::Duration;

/// What a script printed and how it ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShellOutput {
    /// `None` when the script did not exit with a code.
    pub exit_code: Option<i32>,
    pub timed_out: bool,
    /// What it printed, clipped by [`Clip`]: the start and the end.
    pub output: String,
}

impl ShellOutput {
    /// A script that exited with `code` and printed `output`.
    pub fn exited(code: i32, output: &str) -> ShellOutput {
        ShellOutput {
            exit_code: Some(code),
            timed_out: false,
            output: output.to_owned(),
        }
    }
}

/// One machine's shell. The server runs scripts over a node's link and the desktop through a
/// peer. A script may carry a token, so implementations never log it.
#[async_trait]
pub trait Shell: Send + Sync {
    async fn run(&self, script: &str, limit: Duration) -> Result<ShellOutput>;
}

/// The most of the start of a script's output kept.
const HEAD: usize = 64 * 1024;
/// The most of the end kept: a clone says how it finished on its last line.
const TAIL: usize = 8 * 1024;

/// Collects a script's output for [`ShellOutput`], keeping its start and its end.
#[derive(Default)]
pub struct Clip {
    head: String,
    tail: String,
}

impl Clip {
    pub fn push(&mut self, text: &str) {
        let room = HEAD.saturating_sub(self.head.len());
        let mut cut = text.len().min(room);
        while !text.is_char_boundary(cut) {
            cut -= 1;
        }
        self.head.push_str(&text[..cut]);
        self.tail.push_str(&text[cut..]);
        let mut start = self.tail.len().saturating_sub(TAIL);
        while !self.tail.is_char_boundary(start) {
            start += 1;
        }
        self.tail.drain(..start);
    }

    pub fn finish(mut self) -> String {
        self.head.push_str(&self.tail);
        self.head
    }
}

/// `text` as one shell word.
pub fn quote(text: &str) -> String {
    // Only a NUL byte cannot be quoted, and no command can carry one.
    shlex::try_quote(&text.replace('\0', ""))
        .map(|quoted| quoted.into_owned())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn output_keeps_its_start_and_end_on_character_boundaries() {
        let mut clip = Clip::default();
        clip.push(&"a".repeat(HEAD - 1));
        clip.push("éé");
        clip.push(&"b".repeat(TAIL * 2));
        clip.push("end");
        let kept = clip.finish();
        assert!(kept.starts_with("aaa"));
        assert!(kept.ends_with("bbbend"));
        assert!(kept.len() <= HEAD + 1 + TAIL + 1);
    }
}
