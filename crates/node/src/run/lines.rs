//! Reading a child's output a line at a time without holding a line of any
//! length: a progress bar redrawn with `\r` never writes a newline.

use std::io;
use tokio::io::{AsyncBufRead, AsyncBufReadExt};

/// The most one read keeps. A longer line comes back in pieces.
pub(crate) const MAX_PIECE: usize = 64 * 1024;

/// Appends to `buf` up to and including the next newline, or `MAX_PIECE`
/// bytes when the line is longer. Returns how many bytes it read, 0 at the
/// end of the output.
pub(crate) async fn read_piece<R: AsyncBufRead + Unpin>(
    reader: &mut R,
    buf: &mut Vec<u8>,
) -> io::Result<usize> {
    let mut read = 0;

    while read < MAX_PIECE {
        let available = reader.fill_buf().await?;

        if available.is_empty() {
            break;
        }

        let room = available.len().min(MAX_PIECE - read);
        let (used, ended) = match available[..room].iter().position(|&b| b == b'\n') {
            Some(i) => (i + 1, true),
            None => (room, false),
        };
        buf.extend_from_slice(&available[..used]);
        reader.consume(used);
        read += used;

        if ended {
            break;
        }
    }

    Ok(read)
}

#[cfg(test)]
#[path = "../tests/run_lines.rs"]
mod tests;
