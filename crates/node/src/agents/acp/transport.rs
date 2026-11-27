//! Bounded stdio framing for an ACP adapter.

use crate::process::line_reader::{Line, LineReader, MAX_MESSAGE};
use agent_client_protocol::Lines;
use futures::{Sink, Stream, sink, stream};
use std::io;
use tokio::{
    io::{AsyncBufRead, AsyncRead, AsyncWrite, AsyncWriteExt, BufReader},
    sync::mpsc,
};


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
                                crate::text::cut(trimmed, 200)
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

#[cfg(test)]
#[path = "../../tests/agents_acp_transport.rs"]
mod tests;
