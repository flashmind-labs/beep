//! Commands in a node's shell (`shell/1`), on one stream so state carries to the next command.

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
        // Taken out while running, so a cancelled run drops the stream instead of leaving its answer.
        let mut shell = match slot.take() {
            Some(shell) => shell,
            None => self.open_shell().await?,
        };
        let mut kept = Kept::default();
        let done = run(&mut shell, command, limit, &mut |text| kept.push(text)).await?;
        *slot = Some(shell);
        Ok(done.ran(kept))
    }

    /// Runs `command` in its own shell in the home folder; `on_output` gets output as it comes.
    pub async fn exec(
        &self,
        command: &str,
        limit: Duration,
        on_output: &mut (dyn FnMut(&str) + Send),
    ) -> Result<Exited> {
        if !self.offers(PROTOCOL) {
            bail!("{} does not offer a shell", self.name);
        }
        // Dropped at the end, which closes the stream and the shell.
        let mut shell = self.open_shell().await?;
        let done = run(&mut shell, command, limit, on_output).await?;
        Ok(Exited {
            exit_code: done.exit_code,
            timed_out: done.timed_out,
            truncated: done.truncated,
        })
    }

    async fn open_shell(&self) -> Result<ShellLink> {
        let (tx, rx) = self.open_stream(PROTOCOL).await?;
        Ok(ShellLink { tx, rx })
    }
}

/// How a command run by `exec` ended.
#[derive(Debug, PartialEq)]
pub struct Exited {
    /// `None` when it did not finish: it timed out or ended its shell.
    pub exit_code: Option<i32>,
    pub timed_out: bool,
    /// The node dropped output past its limit.
    pub truncated: bool,
}

/// What the node said when a command ended.
struct Done {
    exit_code: Option<i32>,
    timed_out: bool,
    cwd: Option<String>,
    fresh: bool,
    truncated: bool,
}

impl Done {
    fn ran(self, kept: Kept) -> Ran {
        Ran {
            cut: self.truncated || kept.cut,
            output: kept.text(),
            exit_code: self.exit_code,
            timed_out: self.timed_out,
            cwd: self.cwd,
            fresh: self.fresh,
        }
    }
}

async fn run(
    shell: &mut ShellLink,
    command: &str,
    limit: Duration,
    on_output: &mut (dyn FnMut(&str) + Send),
) -> Result<Done> {
    let timeout_secs = u32::try_from(limit.as_secs()).unwrap_or(u32::MAX);
    shell
        .tx
        .send(&Request::Run {
            command: command.to_owned(),
            timeout_secs,
        })
        .await?;
    let answer = async {
        loop {
            match shell.rx.recv::<Event>().await? {
                Some(Event::Output { text }) => on_output(&text),
                Some(Event::Done {
                    exit_code,
                    timed_out,
                    cwd,
                    fresh,
                    truncated,
                }) => {
                    return Ok::<_, anyhow::Error>(Done {
                        exit_code,
                        timed_out,
                        cwd,
                        fresh,
                        truncated,
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
        // Once anything went to the tail, the head is closed.
        let room = if self.tail.is_empty() {
            KEEP_HEAD - self.head.len()
        } else {
            0
        };
        let split = text.floor_char_boundary(room);
        self.head.push_str(&text[..split]);
        self.tail.push_str(&text[split..]);
        if self.tail.len() > KEEP_TAIL * 2 {
            self.trim_tail();
        }
    }

    fn trim_tail(&mut self) {
        if self.tail.len() > KEEP_TAIL {
            let start = self.tail.ceil_char_boundary(self.tail.len() - KEEP_TAIL);
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
    fn a_split_character_keeps_the_output_in_order() {
        let mut kept = Kept::default();
        kept.push(&"a".repeat(KEEP_HEAD - 1));
        kept.push("é");
        kept.push("x");
        assert_eq!(kept.text(), format!("{}éx", "a".repeat(KEEP_HEAD - 1)));
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
