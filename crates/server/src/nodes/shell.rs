//! Running commands in a node's shell (`shell/1`). Each node keeps one shell
//! stream, opened when first needed, so the folder and variables one command
//! sets are there for the next.

use super::Online;
use crate::{
    AsyncMutex,
    protocol::node::{
        LinkRx, LinkTx,
        shell::{Event, PROTOCOL, Request},
    },
};
use anyhow::{Context, Result, bail};
use std::time::Duration;
use tokio::time::timeout;

/// How much output the model gets: the start and, mostly, the end.
const KEEP_HEAD: usize = 4 * 1024;
const KEEP_TAIL: usize = 12 * 1024;
/// How long past its own timeout a command's answer may take.
const GRACE: Duration = Duration::from_secs(15);

/// The node's shell stream, while one is open.
pub type ShellSlot = AsyncMutex<Option<ShellLink>>;

pub struct ShellLink {
    tx: LinkTx,
    rx: LinkRx,
}

/// What a command did.
#[derive(Debug, PartialEq)]
pub struct Ran {
    pub output: String,
    pub exit_code: Option<i32>,
    pub timed_out: bool,
    pub cwd: Option<String>,
    pub fresh: bool,
    /// Output was left out, by the node or here.
    pub cut: bool,
}

impl Online {
    pub fn offers(&self, protocol: &str) -> bool {
        self.offers.iter().any(|offer| offer == protocol)
    }

    /// Runs `command` in the node's shell, after any command already running.
    pub async fn run_shell(&self, command: &str, limit: Duration) -> Result<Ran> {
        if !self.offers(PROTOCOL) {
            bail!("{} does not offer a shell", self.name);
        }
        // Held for the whole command: commands in one shell run in turn.
        let mut slot = self.shell.lock().await;
        // Taken out while the command runs, so a run cut short (its turn was
        // cancelled) drops the stream, and the node stops the shell, instead
        // of leaving its answer for the next command to read.
        let mut shell = match slot.take() {
            Some(shell) => shell,
            None => self.open_shell().await?,
        };
        let ran = run(&mut shell, command, limit).await?;
        *slot = Some(shell);
        Ok(ran)
    }

    async fn open_shell(&self) -> Result<ShellLink> {
        let (tx, rx) = self.open_stream(PROTOCOL).await?;
        Ok(ShellLink { tx, rx })
    }
}

async fn run(shell: &mut ShellLink, command: &str, limit: Duration) -> Result<Ran> {
    let timeout_secs = u32::try_from(limit.as_secs()).unwrap_or(u32::MAX);
    shell
        .tx
        .send(&Request::Run {
            command: command.to_owned(),
            timeout_secs,
        })
        .await?;
    let mut output = Kept::default();
    let answer = async {
        loop {
            match shell.rx.recv::<Event>().await? {
                Some(Event::Output { text }) => output.push(&text),
                Some(Event::Done {
                    exit_code,
                    timed_out,
                    cwd,
                    fresh,
                    truncated,
                }) => {
                    return Ok::<_, anyhow::Error>(Ran {
                        cut: truncated || output.cut,
                        output: output.text(),
                        exit_code,
                        timed_out,
                        cwd,
                        fresh,
                    });
                }
                Some(other) => log::debug!("shell sent something unhandled: {other:?}"),
                None => bail!("the node closed the shell"),
            }
        }
    };
    timeout(limit + GRACE, answer)
        .await
        .context("the node stopped answering")?
}

/// The first `KEEP_HEAD` bytes of the output and the last `KEEP_TAIL`.
#[derive(Default)]
struct Kept {
    head: String,
    tail: String,
    cut: bool,
}

impl Kept {
    fn push(&mut self, text: &str) {
        let room = KEEP_HEAD - self.head.len();
        let split = floor_boundary(text, room.min(text.len()));
        self.head.push_str(&text[..split]);
        self.tail.push_str(&text[split..]);
        if self.tail.len() > KEEP_TAIL * 2 {
            self.trim_tail();
        }
    }

    fn trim_tail(&mut self) {
        if self.tail.len() > KEEP_TAIL {
            let start = ceil_boundary(&self.tail, self.tail.len() - KEEP_TAIL);
            self.tail.drain(..start);
            self.cut = true;
        }
    }

    fn text(mut self) -> String {
        self.trim_tail();
        if self.cut {
            format!("{}\n[... output cut ...]\n{}", self.head, self.tail)
        } else {
            self.head + &self.tail
        }
    }
}

/// `text` cut the way shell output is: its start and its end, with a note
/// where the middle went.
pub(crate) fn head_and_tail(text: &str) -> String {
    let mut kept = Kept::default();
    kept.push(text);
    kept.text()
}

fn floor_boundary(text: &str, mut at: usize) -> usize {
    while !text.is_char_boundary(at) {
        at -= 1;
    }
    at
}

fn ceil_boundary(text: &str, mut at: usize) -> usize {
    while !text.is_char_boundary(at) {
        at += 1;
    }
    at
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_output_is_kept_whole() {
        let mut kept = Kept::default();
        kept.push("hello ");
        kept.push("world");
        assert!(!kept.cut);
        assert_eq!(kept.text(), "hello world");
    }

    #[test]
    fn long_output_keeps_its_start_and_end() {
        let mut kept = Kept::default();
        kept.push("start\n");
        for _ in 0..10_000 {
            kept.push("é line of noise\n");
        }
        kept.push("the end");
        let text = kept.text();
        assert!(text.starts_with("start\n"));
        assert!(text.contains("[... output cut ...]"));
        assert!(text.ends_with("the end"));
        assert!(text.len() < KEEP_HEAD + KEEP_TAIL + 64);
    }
}
