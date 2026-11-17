//! Bounded stdio framing for an ACP adapter.

use agent_client_protocol::Lines;
use futures::{Sink, Stream, sink, stream};
use std::io;
use tokio::{
    io::{AsyncBufRead, AsyncBufReadExt, AsyncRead, AsyncWrite, AsyncWriteExt, BufReader},
    sync::mpsc,
};

/// The longest message read from an agent; a longer one is dropped.
pub(crate) const MAX_MESSAGE: usize = 16 * 1024 * 1024;

/// The transport over an adapter's stdio, or the node's own when it is the
/// agent. The crate writes from its own actor, apart from reading, so a full
/// stdin never stops stdout from being read. Each oversized message dropped
/// is reported on `oversized` with its length.
pub(in crate::agents) fn transport(
    stdin: impl AsyncWrite + Unpin + Send + 'static,
    stdout: impl AsyncRead + Unpin + Send + 'static,
    oversized: mpsc::Sender<usize>,
) -> Lines<
    impl Sink<String, Error = io::Error> + Send + 'static,
    impl Stream<Item = io::Result<String>> + Send + 'static,
> {
    Lines::new(
        writer(stdin),
        messages(BufReader::new(stdout), MAX_MESSAGE, oversized),
    )
}

/// Writes each message the crate hands it as one line.
fn writer<W: AsyncWrite + Unpin + Send + 'static>(
    stdin: W,
) -> impl Sink<String, Error = io::Error> + Send + 'static {
    Box::pin(sink::unfold(stdin, async |mut stdin, message: String| {
        stdin.write_all(message.as_bytes()).await?;
        stdin.write_all(b"\n").await?;
        stdin.flush().await?;
        Ok::<_, io::Error>(stdin)
    }))
}

/// The JSON-RPC messages in `reader`, one per line, each at most `max`
/// bytes. Other lines (an npx warning, say) are logged and skipped rather
/// than answered with a parse error.
fn messages<R: AsyncBufRead + Unpin + Send + 'static>(
    reader: R,
    max: usize,
    oversized: mpsc::Sender<usize>,
) -> impl Stream<Item = io::Result<String>> + Send + 'static {
    let lines = LineReader::new(reader, max);
    stream::unfold(lines, move |mut lines| {
        let oversized = oversized.clone();
        async move {
            loop {
                match lines.next().await {
                    Ok(Some(Line::Text(text))) => {
                        let trimmed = text.trim_start();

                        if trimmed.starts_with('{') || trimmed.starts_with('[') {
                            return Some((Ok(text), lines));
                        }

                        if !trimmed.is_empty() {
                            log::debug!(
                                "agent wrote a line that is not JSON-RPC: {}",
                                crate::agents::cut(trimmed, 200)
                            );
                        }
                    }
                    Ok(Some(Line::TooLong(length))) => {
                        log::warn!("dropped a {length} byte message from the agent: over {max}");
                        // A full channel already carries a report.
                        let _ = oversized.try_send(length);
                    }
                    Ok(None) => return None,
                    Err(e) => return Some((Err(e), lines)),
                }
            }
        }
    })
}

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
#[path = "../../tests/agents_acp_transport.rs"]
mod tests;
