//! The terminal host, `beep-node pty-host` (`run`): a process of its own
//! that keeps the node's pseudo-terminals so they outlive it, as dtach does.

use super::{
    serve::{Peer, serve_with},
    terminals::{OpenTerminal, Terminals, unknown},
};
use crate::{
    AsyncMutex, lock,
    run::local_socket::{Listener, Stream, connect, is_listening},
    files::note,
    machine::spawn_env::{self, Scope},
};
use anyhow::{Context, Result, bail};
use beep_proto::{
    SecretText, lenient, watched,
    node::{
        EnvSecret, MAX_FRAME,
        terminal::{Reply, Request},
    },
};
use futures::{SinkExt, StreamExt};
use notify::{EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use std::{
    io,
    path::{Path, PathBuf},
    process::Stdio,
    sync::{
        Mutex, OnceLock,
        atomic::{AtomicUsize, Ordering},
    },
    time::{Duration, Instant},
};
use tokio::{process::Command, sync::Notify, time::timeout};
use tokio_util::{
    bytes::Bytes,
    codec::{Framed, LengthDelimitedCodec},
};

/// The host's socket, in the node's folder. The node starts the host
/// detached for its first terminal and finds it here after a restart.
const SOCKET: &str = "terminals.sock";
/// What runs, in the node's folder. When the host died with its
/// terminals, the next host opens them again as new shells once a node
/// says hello.
const NOTE: &str = "terminals.json";
/// How long the host stays with no terminal and no node.
const IDLE_EXIT: Duration = Duration::from_secs(10);
/// How long a node waits for a host it started to listen.
const START_WAIT: Duration = Duration::from_secs(10);
/// How often it looks meanwhile.
const START_POLL: Duration = Duration::from_millis(25);
/// The most connections served at once: one per stream of the caller's.
const MAX_CONNECTIONS: usize = 64;

/// The node's folder, where its host listens.
static DIR: OnceLock<PathBuf> = OnceLock::new();
/// The host writes its running set after every lifecycle change.
static CHANGES: OnceLock<RecommendedWatcher> = OnceLock::new();
/// Held while the node starts a host, so it starts one.
static STARTING: AsyncMutex<()> = AsyncMutex::const_new(());

/// What a node says first on each connection, before `terminal/1`, so a
/// host left by an older node serves a newer one.
#[derive(Serialize, Deserialize)]
struct Hello {
    /// What terminals started from now on get on top of the host's own
    /// environment: the node's tool folders and the user's credentials.
    #[serde(default)]
    env: Vec<EnvSecret>,
}

/// One connection between the node and the host, either side of it.
struct Conn(Framed<Stream, LengthDelimitedCodec>);

impl Conn {
    fn new(stream: Stream) -> Self {
        let codec = LengthDelimitedCodec::builder()
            .max_frame_length(MAX_FRAME)
            .new_codec();
        Self(Framed::new(stream, codec))
    }

    async fn write<T: Serialize>(&mut self, message: &T) -> Result<()> {
        // A host from a released node outlives it and reads with plain
        // `rmp_serde`, which refuses a longer array: these messages gain no
        // field while such a host can run (AGENTS.md).
        let body = rmp_serde::to_vec(message)?;
        self.0.send(Into::<Bytes>::into(body)).await?;
        Ok(())
    }

    async fn read<T: DeserializeOwned>(&mut self) -> Result<Option<T>> {
        let Some(frame) = self.0.next().await else {
            return Ok(None);
        };
        let message = lenient::from_slice(&frame?).context("could not read a terminal message")?;
        Ok(Some(message))
    }
}

impl Peer for Conn {
    async fn send(&mut self, reply: &Reply) -> Result<()> {
        self.write(reply).await
    }

    async fn recv(&mut self) -> Result<Option<Request>> {
        self.read().await
    }
}

/// Runs the host for the node in `dir` until no terminal runs and no node
/// is connected. Leaves at once when another host listens there.
pub async fn run(dir: &Path) -> Result<()> {
    let socket = dir.join(SOCKET);

    if is_listening(&socket) {
        log::info!("a terminal host already runs");
        return Ok(());
    }

    let listener = Listener::bind(&socket)?;
    serve(dir, listener, &Terminals::default()).await
}

/// Binds a host for the node in `dir` in this process and returns it to
/// run, for a program that embeds the node with no `beep-node` binary to
/// start one, such as a test.
pub fn in_process(dir: &Path) -> Result<impl Future<Output = Result<()>> + use<>> {
    if DIR.set(dir.to_path_buf()).is_err() {
        bail!("the node's terminals were set up already");
    }

    watch(dir)?;
    let listener = Listener::bind(&dir.join(SOCKET))?;
    let dir = dir.to_path_buf();
    Ok(async move { serve(&dir, listener, &Terminals::default()).await })
}

/// What the host's connections share.
struct Host<'a> {
    terminals: &'a Terminals,
    connections: AtomicUsize,
    /// Woken when a connection ends.
    disconnected: Notify,
    /// Terminals the last host died with, until a node says hello.
    noted: Mutex<Option<Vec<OpenTerminal>>>,
}

impl Host<'_> {
    fn idle(&self) -> bool {
        self.connections.load(Ordering::Relaxed) == 0
            && !self.terminals.any_running()
    }
}

/// Counts a connection while it lasts.
struct Connected<'a>(&'a Host<'a>);

impl<'a> Connected<'a> {
    fn new(host: &'a Host<'a>) -> Self {
        host.connections.fetch_add(1, Ordering::Relaxed);
        Self(host)
    }
}

impl Drop for Connected<'_> {
    fn drop(&mut self) {
        self.0.connections.fetch_sub(1, Ordering::Relaxed);
        self.0.disconnected.notify_one();
    }
}

async fn serve(dir: &Path, mut listener: Listener, terminals: &Terminals) -> Result<()> {
    let noted = read_note(dir);
    let host = Host {
        terminals,
        connections: AtomicUsize::new(0),
        disconnected: Notify::new(),
        noted: Mutex::new((!noted.is_empty()).then_some(noted)),
    };
    let keeping = keep(dir, &host);
    tokio::pin!(keeping);
    // Each connection borrows the host, so they run here, not spawned.
    let mut connections = futures::stream::FuturesUnordered::new();

    loop {
        tokio::select! {
            () = &mut keeping => return Ok(()),
            stream = listener.accept() => {
                let stream = stream.context("taking a node's connection")?;

                if connections.len() >= MAX_CONNECTIONS {
                    log::warn!("refused a connection: {MAX_CONNECTIONS} are open");
                    continue;
                }

                connections.push(connection(stream, &host));
            }
            Some(()) = connections.next() => {}
        }
    }
}

/// Serves one node's connection: its hello, then `terminal/1`.
async fn connection(stream: Stream, host: &Host<'_>) {
    let _connected = Connected::new(host);
    let mut conn = Conn::new(stream);
    let served = async {
        let Some(hello) = conn.read::<Hello>().await? else {
            return Ok(());
        };
        host.terminals.set_env(hello.env);
        // Opened only now, with the node's environment.
        let noted = lock(&host.noted).take();

        for open in noted.into_iter().flatten() {
            let id = open.id;

            if let Err(e) = host.terminals.reopen(open).await {
                log::warn!("could not reopen terminal {id}: {e:#}");
            }
        }

        serve_with(host.terminals, &mut conn).await
    };

    if let Err(e) = served.await {
        log::debug!("a node's connection failed: {e:#}");
    }
}

/// Keeps the note to what runs, and returns once the host stayed idle for
/// `IDLE_EXIT`.
async fn keep(dir: &Path, host: &Host<'_>) {
    let mut terminals = host.terminals.subscribe();

    loop {
        // Terminals waiting to open again stay noted as they were.
        if lock(&host.noted).is_none()
            && let Err(e) = note::write_json(&dir.join(NOTE), &host.terminals.running())
        {
            log::warn!("terminals would not open again after a reboot: {e:#}");
        }

        // Marked seen before the check, so a change after it wakes the wait.
        watched::read_and_update(&mut terminals, |_| ());
        let change = async {
            tokio::select! {
                _ = terminals.changed() => {}
                () = host.disconnected.notified() => {}
            }
        };
        if !host.idle() {
            change.await;
        } else if timeout(IDLE_EXIT, change).await.is_err() && host.idle() {
            log::info!("no terminal runs; the terminal host exits");
            return;
        }
    }
}

fn read_note(dir: &Path) -> Vec<OpenTerminal> {
    note::read(&dir.join(NOTE))
}

fn watch(dir: &Path) -> Result<()> {
    if CHANGES.get().is_some() {
        return Ok(());
    }

    let dir = dir.canonicalize()?;
    let path = dir.join(NOTE);
    let mut current = read_note(&dir);
    let mut watcher = notify::recommended_watcher(move |event: notify::Result<notify::Event>| {
        crate::events::paths::counted("terminal-host", &event);

        match event {
            Ok(event)
                if !event.need_rescan()
                    && (matches!(event.kind, EventKind::Access(_) | EventKind::Other)
                        || !event.paths.contains(&path)) =>
            {
                return;
            }
            Err(error) => log::warn!("terminal host notification failed: {error}"),
            _ => {}
        }

        let next: Vec<OpenTerminal> = note::read(&path);
        // Lists also rewrite the note, so unchanged contents must not trigger another list.
        if next != current {
            current = next;
            crate::events::system().changed(beep_proto::node::events::Topic::new("terminals"));
        }
    })?;
    watcher.watch(&dir, RecursiveMode::NonRecursive)?;
    let _ = CHANGES.set(watcher);
    Ok(())
}

/// Remembers the node's folder for `relay`, and starts a host there when
/// the last one died with terminals it noted, which it opens again.
pub(super) async fn reopen(dir: &Path) {
    if DIR.set(dir.to_path_buf()).is_err() {
        log::debug!("the node's terminals were set up already");
    }

    if let Err(error) = watch(dir) {
        log::warn!("could not watch terminal host changes: {error:#}");
    }

    if read_note(dir).is_empty() || is_listening(&dir.join(SOCKET)) {
        return;
    }

    match open(dir, true).await {
        Ok(_) => log::info!("the terminals that were open start again as new shells"),
        Err(e) => log::warn!("could not open the terminals again: {e:#}"),
    }
}

/// Relays one session stream to the host until either side closes it.
pub(super) async fn relay(link: &mut impl Peer) -> Result<()> {
    let dir = DIR
        .get()
        .context("the node's terminals are not set up yet")?;
    relay_in(dir, link).await
}

/// What came first, while relaying.
enum Relayed {
    Request(Option<Request>),
    Reply(Option<Reply>),
}

async fn relay_in(dir: &Path, link: &mut impl Peer) -> Result<()> {
    let Some(first) = link.recv().await? else {
        return Ok(());
    };
    let start = matches!(first, Request::Start { .. });
    let mut host = match open(dir, start).await {
        Ok(Some(host)) => host,
        // No host runs, so no terminal does.
        Ok(None) => {
            let reply = match first {
                Request::List {} => Reply::Terminals {
                    terminals: Vec::new(),
                },
                Request::Attach { id, .. } | Request::Stop { id } => unknown(id),
                other => Reply::Error {
                    message: format!("{other:?} needs an attached terminal"),
                },
            };
            return link.send(&reply).await;
        }
        Err(e) => {
            let message = format!("Could not start the terminal host: {e:#}");
            return link.send(&Reply::Error { message }).await;
        }
    };
    host.write(&first).await?;

    loop {
        let relayed = tokio::select! {
            request = link.recv() => Relayed::Request(request?),
            reply = host.read() => Relayed::Reply(reply?),
        };

        match relayed {
            Relayed::Request(Some(request)) => host.write(&request).await?,
            Relayed::Reply(Some(reply)) => link.send(&reply).await?,
            // One side closed; dropping the other closes it too.
            Relayed::Request(None) | Relayed::Reply(None) => return Ok(()),
        }
    }
}

/// A hello'd connection to the host for the node in `dir`, starting one
/// when `start` and none runs; `None` when none runs and not `start`.
async fn open(dir: &Path, start: bool) -> Result<Option<Conn>> {
    let socket = dir.join(SOCKET);
    let stream = match connect(&socket).await {
        Ok(stream) => stream,
        Err(_) if !start => return Ok(None),
        Err(_) => launch(dir).await?,
    };
    let mut conn = Conn::new(stream);
    let env = spawn_env::vars(&Scope::Shell)
        .into_iter()
        .filter_map(|(name, value)| {
            Some(EnvSecret {
                name: name.into_string().ok()?,
                value: SecretText::new(value.into_string().ok()?),
            })
        })
        .collect();
    conn.write(&Hello { env }).await?;
    Ok(Some(conn))
}

/// Starts a host for the node in `dir`, unless one started meanwhile, and
/// connects to it.
async fn launch(dir: &Path) -> Result<Stream> {
    let _starting = STARTING.lock().await;
    let socket = dir.join(SOCKET);

    if let Ok(stream) = connect(&socket).await {
        return Ok(stream);
    }

    let exe = std::env::current_exe().context("finding beep-node")?;
    let mut failed = None;

    for mut command in launchers(&exe, dir) {
        command
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        // SAFETY: setsid only changes the child's session, and is safe to
        // call between fork and exec.
        unsafe {
            command.pre_exec(|| rustix::process::setsid().map(drop).map_err(io::Error::from));
        }
        match listening(command, &socket).await {
            Ok(stream) => return Ok(stream),
            Err(e) => {
                log::debug!("the terminal host did not start: {e:#}");
                failed = Some(e);
            }
        }
    }

    Err(failed.unwrap_or_else(|| anyhow::anyhow!("no way to start the terminal host")))
}

/// The ways to start the host, best first. Under systemd it goes in a
/// scope of its own, since stopping the node's unit ends all of its
/// processes; its own session keeps it from the node's terminal and
/// process group elsewhere.
fn launchers(exe: &Path, dir: &Path) -> Vec<Command> {
    let host = |program: &Path| {
        let mut command = Command::new(program);
        command.arg("pty-host").arg("--dir").arg(dir);
        command
    };
    let mut launchers = Vec::new();

    if std::env::var_os("INVOCATION_ID").is_some()
        && let Ok(systemd_run) = which::which("systemd-run")
    {
        let mut command = Command::new(systemd_run);
        command
            .args(["--user", "--scope", "--quiet", "--collect", "--"])
            .arg(exe)
            .arg("pty-host")
            .arg("--dir")
            .arg(dir);
        launchers.push(command);
    }

    launchers.push(host(exe));
    launchers
}

/// Runs `command` and connects once it listens on `socket`.
async fn listening(mut command: Command, socket: &Path) -> Result<Stream> {
    let mut child = command.spawn().context("starting the terminal host")?;
    let deadline = Instant::now() + START_WAIT;

    loop {
        if let Ok(stream) = connect(socket).await {
            return Ok(stream);
        }

        if let Some(status) = child.try_wait()? {
            bail!("it exited: {status}");
        }

        if Instant::now() >= deadline {
            if let Err(e) = child.start_kill() {
                log::debug!("could not stop the terminal host: {e}");
            }

            bail!("it did not listen in {}s", START_WAIT.as_secs());
        }

        tokio::time::sleep(START_POLL).await;
    }
}

#[cfg(test)]
#[path = "../../tests/run_terminal_host.rs"]
mod tests;
