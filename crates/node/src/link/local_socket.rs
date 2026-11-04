//! The local sockets hooks and the MCP server reach the node on: a Unix
//! socket in the node's folder, or on Windows a named pipe whose name
//! comes from that path.

use anyhow::{Context, Result};
use std::{io, path::Path};

#[cfg(unix)]
pub(crate) use unix::{Listener, Stream, connect, is_listening};
#[cfg(windows)]
pub(crate) use windows::{Listener, Stream, connect, is_listening};

#[cfg(unix)]
mod unix {
    use super::*;
    use crate::files::fsat;
    use std::fs;
    use tokio::net::{UnixListener, UnixStream};

    pub(crate) type Stream = UnixStream;

    pub(crate) struct Listener(UnixListener);

    impl Listener {
        /// Listens on `socket`, replacing one a stopped node left.
        pub(crate) fn bind(socket: &Path) -> Result<Self> {
            match fs::remove_file(socket) {
                Ok(()) => {}
                Err(e) if e.kind() == io::ErrorKind::NotFound => {}
                Err(e) => return Err(e).with_context(|| format!("removing {}", socket.display())),
            }
            let listener = UnixListener::bind(socket)
                .with_context(|| format!("listening on {}", socket.display()))?;
            fsat::set_mode(socket, 0o600)?;
            Ok(Self(listener))
        }

        pub(crate) async fn accept(&mut self) -> io::Result<Stream> {
            Ok(self.0.accept().await?.0)
        }
    }

    pub(crate) async fn connect(socket: &Path) -> io::Result<Stream> {
        UnixStream::connect(socket).await
    }

    pub(crate) fn is_listening(socket: &Path) -> bool {
        std::os::unix::net::UnixStream::connect(socket).is_ok()
    }
}

#[cfg(windows)]
mod windows {
    use super::*;
    use std::{
        mem,
        pin::Pin,
        task::{self, Poll},
        time::{Duration, Instant},
    };
    use tokio::{
        io::{AsyncRead, AsyncWrite, ReadBuf},
        time::sleep,
        net::windows::named_pipe::{
            ClientOptions, NamedPipeClient, NamedPipeServer, ServerOptions,
        },
    };

    /// What a connection reads from and writes to.
    pub(crate) enum Stream {
        Server(NamedPipeServer),
        Client(NamedPipeClient),
    }

    impl AsyncRead for Stream {
        fn poll_read(
            self: Pin<&mut Self>,
            cx: &mut task::Context<'_>,
            buf: &mut ReadBuf<'_>,
        ) -> Poll<io::Result<()>> {
            match self.get_mut() {
                Self::Server(pipe) => Pin::new(pipe).poll_read(cx, buf),
                Self::Client(pipe) => Pin::new(pipe).poll_read(cx, buf),
            }
        }
    }

    impl AsyncWrite for Stream {
        fn poll_write(
            self: Pin<&mut Self>,
            cx: &mut task::Context<'_>,
            buf: &[u8],
        ) -> Poll<io::Result<usize>> {
            match self.get_mut() {
                Self::Server(pipe) => Pin::new(pipe).poll_write(cx, buf),
                Self::Client(pipe) => Pin::new(pipe).poll_write(cx, buf),
            }
        }

        fn poll_flush(self: Pin<&mut Self>, cx: &mut task::Context<'_>) -> Poll<io::Result<()>> {
            match self.get_mut() {
                Self::Server(pipe) => Pin::new(pipe).poll_flush(cx),
                Self::Client(pipe) => Pin::new(pipe).poll_flush(cx),
            }
        }

        fn poll_shutdown(self: Pin<&mut Self>, cx: &mut task::Context<'_>) -> Poll<io::Result<()>> {
            match self.get_mut() {
                Self::Server(pipe) => Pin::new(pipe).poll_shutdown(cx),
                Self::Client(pipe) => Pin::new(pipe).poll_shutdown(cx),
            }
        }
    }

    /// The pipe for `socket`: one per node folder, named by a hash of its
    /// path.
    pub(super) fn pipe_name(socket: &Path) -> String {
        let hash = blake3::hash(socket.as_os_str().as_encoded_bytes());
        format!(r"\\.\pipe\beep-node-{}", &hash.to_hex()[..32])
    }

    /// A pipe instance waiting for the next hook. The default security
    /// lets only this user (and administrators) write to it, and remote
    /// clients are refused.
    pub(crate) struct Listener {
        name: String,
        next: NamedPipeServer,
    }

    impl Listener {
        pub(crate) fn bind(socket: &Path) -> Result<Self> {
            let name = pipe_name(socket);
            let next = ServerOptions::new()
                .first_pipe_instance(true)
                .reject_remote_clients(true)
                .create(&name)
                .with_context(|| format!("listening on {name}"))?;
            Ok(Self { name, next })
        }

        pub(crate) async fn accept(&mut self) -> io::Result<Stream> {
            self.next.connect().await?;
            let fresh = ServerOptions::new()
                .reject_remote_clients(true)
                .create(&self.name)?;
            Ok(Stream::Server(mem::replace(&mut self.next, fresh)))
        }
    }

    /// `ERROR_PIPE_BUSY`: every instance has a client and the node has not
    /// made the next one yet.
    const PIPE_BUSY: i32 = 231;
    /// How long a connection waits for the node to make the next instance.
    const BUSY_LIMIT: Duration = Duration::from_secs(5);

    pub(crate) async fn connect(socket: &Path) -> io::Result<Stream> {
        let name = pipe_name(socket);
        let start = Instant::now();
        loop {
            match ClientOptions::new().open(&name) {
                Ok(client) => return Ok(Stream::Client(client)),
                Err(e) if e.raw_os_error() == Some(PIPE_BUSY) && start.elapsed() < BUSY_LIMIT => {
                    sleep(Duration::from_millis(10)).await;
                }
                Err(e) => return Err(e),
            }
        }
    }

    /// Whether a node has the pipe open. Opening it takes one connection,
    /// which the node drops as an empty message.
    pub(crate) fn is_listening(socket: &Path) -> bool {
        std::fs::OpenOptions::new()
            .write(true)
            .open(pipe_name(socket))
            .is_ok()
    }
}

/// Where hooks connect, for messages: the socket, or the pipe it names.
pub(crate) fn shown(socket: &Path) -> String {
    #[cfg(windows)]
    return windows::pipe_name(socket);
    #[cfg(unix)]
    socket.display().to_string()
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;
    use tokio::io::AsyncWriteExt;

    /// `mesh join` checks that a node listens, waits for its socket, then
    /// connects to send the ticket: three connections in a row, each of
    /// which may find the node between two pipe instances.
    #[tokio::test]
    async fn connections_in_a_row_each_reach_the_node() {
        let socket = std::env::temp_dir().join(format!("beep-pipe-test-{}", std::process::id()));
        let mut listener = Listener::bind(&socket).expect("the pipe is made");
        tokio::spawn(async move {
            while let Ok(stream) = listener.accept().await {
                drop(stream);
            }
        });
        assert!(is_listening(&socket));
        let mut first = connect(&socket).await.expect("the second connection reaches the node");
        first.shutdown().await.ok();
        connect(&socket).await.expect("the third connection reaches the node");
    }
}
