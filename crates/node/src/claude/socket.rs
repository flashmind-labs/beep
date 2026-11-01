//! The local socket hooks reach the node on: a Unix socket in the node's
//! folder, or on Windows a named pipe whose name comes from that path.

use anyhow::{Context, Result};
use std::{io, path::Path};

#[cfg(unix)]
pub(super) use unix::{Listener, Stream, connect, is_listening};
#[cfg(windows)]
pub(super) use windows::{Listener, Stream, connect, is_listening};

#[cfg(unix)]
mod unix {
    use super::*;
    use crate::fsat;
    use std::fs;
    use tokio::net::{UnixListener, UnixStream};

    pub(in crate::claude) type Stream = UnixStream;

    pub(in crate::claude) struct Listener(UnixListener);

    impl Listener {
        /// Listens on `socket`, replacing one a stopped node left.
        pub(in crate::claude) fn bind(socket: &Path) -> Result<Self> {
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

        pub(in crate::claude) async fn accept(&mut self) -> io::Result<Stream> {
            Ok(self.0.accept().await?.0)
        }
    }

    pub(in crate::claude) async fn connect(socket: &Path) -> io::Result<Stream> {
        UnixStream::connect(socket).await
    }

    pub(in crate::claude) fn is_listening(socket: &Path) -> bool {
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
    };
    use tokio::{
        io::{AsyncRead, AsyncWrite, ReadBuf},
        net::windows::named_pipe::{
            ClientOptions, NamedPipeClient, NamedPipeServer, ServerOptions,
        },
    };

    /// What a connection reads from and writes to.
    pub(in crate::claude) enum Stream {
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
    pub(in crate::claude) struct Listener {
        name: String,
        next: NamedPipeServer,
    }

    impl Listener {
        pub(in crate::claude) fn bind(socket: &Path) -> Result<Self> {
            let name = pipe_name(socket);
            let next = ServerOptions::new()
                .first_pipe_instance(true)
                .reject_remote_clients(true)
                .create(&name)
                .with_context(|| format!("listening on {name}"))?;
            Ok(Self { name, next })
        }

        pub(in crate::claude) async fn accept(&mut self) -> io::Result<Stream> {
            self.next.connect().await?;
            let fresh = ServerOptions::new()
                .reject_remote_clients(true)
                .create(&self.name)?;
            Ok(Stream::Server(mem::replace(&mut self.next, fresh)))
        }
    }

    pub(in crate::claude) async fn connect(socket: &Path) -> io::Result<Stream> {
        ClientOptions::new()
            .open(pipe_name(socket))
            .map(Stream::Client)
    }

    /// Whether a node has the pipe open. Opening it takes one connection,
    /// which the node drops as an empty message.
    pub(in crate::claude) fn is_listening(socket: &Path) -> bool {
        std::fs::OpenOptions::new()
            .write(true)
            .open(pipe_name(socket))
            .is_ok()
    }
}

/// Where hooks connect, for messages: the socket, or the pipe it names.
pub(super) fn shown(socket: &Path) -> String {
    #[cfg(windows)]
    return windows::pipe_name(socket);
    #[cfg(unix)]
    socket.display().to_string()
}
