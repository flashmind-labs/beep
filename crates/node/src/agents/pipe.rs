//! The CLI a Direct bridge drives: JSON lines written to its stdin and read
//! from its stdout, with paths turned across when it runs in WSL.

use super::launch::{Cli, Process};
use crate::process::line_reader::{Line, LineReader, MAX_MESSAGE};
use crate::machine::wsl::{self, Toward};
use crate::process::STOP_GRACE;
use anyhow::Result;
use serde_json::Value;
use tokio::{
    io::{AsyncWriteExt, BufReader},
    process::Command,
    sync::mpsc,
};
use tokio_util::task::AbortOnDropHandle;

/// How many frames wait to be handled before the reader stops reading.
const FRAMES: usize = 256;

/// A line the CLI wrote.
pub(super) enum Frame {
    Json(Value),
    /// A line over `MAX_MESSAGE`, dropped.
    TooLong,
}

/// A running CLI. Dropping it kills its process group.
pub(super) struct Pipe {
    lines: mpsc::UnboundedSender<String>,
    /// It runs in WSL: the paths in what it is sent and what it writes are
    /// turned across.
    in_wsl: bool,
    /// Its stdout, one frame each; closed when it exits.
    pub(super) frames: mpsc::Receiver<Frame>,
    process: Process,
    _tasks: [AbortOnDropHandle<()>; 2],
}

impl Pipe {
    /// Starts `process`, the agent `name` on `cli`, which runs in WSL when
    /// `in_wsl`. Its stderr is the agent's, as the session reads it.
    pub(super) fn start(cli: &mut Cli, process: Command, name: &'static str, in_wsl: bool) -> Result<Self> {
        let (process, mut stdin, stdout) = cli.spawn(process, name)?;
        let (lines, mut written) = mpsc::unbounded_channel::<String>();
        let (sent, frames) = mpsc::channel(FRAMES);

        let writer = tokio::spawn(async move {
            while let Some(line) = written.recv().await {
                let wrote = async {
                    stdin.write_all(line.as_bytes()).await?;
                    stdin.write_all(b"\n").await?;
                    stdin.flush().await
                };
                if let Err(e) = wrote.await {
                    log::warn!("could not write to {name}: {e}");
                    return;
                }
            }
        });
        let reader = tokio::spawn(async move {
            let mut lines = LineReader::new(BufReader::new(stdout), MAX_MESSAGE);

            while let Ok(Some(line)) = lines.next().await {
                let text = match line {
                    Line::Text(text) => text,
                    Line::TooLong(length) => {
                        log::warn!("dropped a {length} byte line from {name}");

                        if sent.send(Frame::TooLong).await.is_err() {
                            return;
                        }
                        continue;
                    }
                };

                match serde_json::from_str::<Value>(&text) {
                    Ok(mut frame) => {
                        if in_wsl {
                            wsl::translate(&mut frame, Toward::Windows);
                        }

                        if sent.send(Frame::Json(frame)).await.is_err() {
                            return;
                        }
                    }
                    Err(_) if text.trim().is_empty() => {}
                    Err(_) => log::debug!("{name} wrote: {}", crate::text::cut(&text, 200)),
                }
            }
        });

        Ok(Self {
            lines,
            in_wsl,
            frames,
            process,
            _tasks: [AbortOnDropHandle::new(writer), AbortOnDropHandle::new(reader)],
        })
    }

    /// Writes `line`. A closed stdin shows as the process's exit on stdout.
    pub(super) fn write(&self, line: &Value) {
        let line = match self.in_wsl {
            true => {
                let mut line = line.clone();
                wsl::translate(&mut line, Toward::Linux);
                line.to_string()
            }
            false => line.to_string(),
        };
        let _ = self.lines.send(line);
    }

    /// Ends the CLI as a session ends its agent: its stdin closes and its
    /// group is asked to end, then killed after `STOP_GRACE`.
    pub(super) async fn stop(self) {
        let Self { lines, process, _tasks, .. } = self;
        drop(lines);
        process.stop(STOP_GRACE).await;
    }
}
