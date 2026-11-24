//! One running language server child process speaking LSP over stdio.

use super::protocol::{encode_frame, file_uri, read_frame, server_request_reply};
use crate::{AsyncMutex, agents::locate, machine::provision::spawn_env};
use anyhow::{Context, Result, bail};
use serde_json::{Value, json};
use std::{
    collections::HashMap,
    env,
    path::{Path, PathBuf},
    process::Stdio,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicI64, Ordering},
    },
    time::Duration,
};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    process::{Child, Command},
    sync::{mpsc, oneshot},
    time::{Instant, timeout},
};

/// How long the initial `initialize` handshake may take.
const INIT_TIMEOUT: Duration = Duration::from_secs(15);
/// How long one `textDocument/definition` call may wait.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(12);
/// LSP error codes where retrying once after a brief pause often succeeds.
pub(crate) const ERR_CONTENT_MODIFIED: i64 = -32801;
pub(crate) const ERR_SERVER_CANCELLED: i64 = -32802;

/// A JSON-RPC error returned by a language server.
#[derive(Debug, Clone)]
pub(crate) struct RpcError {
    pub(crate) code: i64,
    pub(crate) message: String,
}

impl std::fmt::Display for RpcError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "LSP error {}: {}", self.code, self.message)
    }
}

impl std::error::Error for RpcError {}

type PendingMap = Arc<Mutex<HashMap<i64, oneshot::Sender<std::result::Result<Value, RpcError>>>>>;

struct OpenDoc {
    version: i32,
    hash: [u8; 32],
}

/// One live language server process bound to a workspace root.
pub(crate) struct Client {
    root: PathBuf,
    writer: mpsc::UnboundedSender<Vec<u8>>,
    pending: PendingMap,
    next_id: AtomicI64,
    alive: Arc<AtomicBool>,
    open_docs: AsyncMutex<HashMap<PathBuf, OpenDoc>>,
    last_used: Mutex<Instant>,
    child: AsyncMutex<Option<Child>>,
}

impl Client {
    /// Spawns `binary` with `args` in `root` and completes the LSP `initialize` handshake.
    pub(crate) async fn spawn(root: &Path, binary: &Path, args: &[&str]) -> Result<Arc<Self>> {
        let mut command = Command::new(binary);
        command
            .args(args)
            .current_dir(root)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);

        if let Some(path) = lsp_path(binary) {
            command.env("PATH", path);
        }

        let mut child = command
            .spawn()
            .with_context(|| format!("starting language server {}", binary.display()))?;
        let mut stdin = child.stdin.take().context("language server missing stdin")?;
        let stdout = child
            .stdout
            .take()
            .context("language server missing stdout")?;
        let stderr = child
            .stderr
            .take()
            .context("language server missing stderr")?;

        let (writer, mut outgoing) = mpsc::unbounded_channel::<Vec<u8>>();
        let pending: PendingMap = Arc::new(Mutex::new(HashMap::new()));
        let alive = Arc::new(AtomicBool::new(true));

        let root_uri = file_uri(root)?;
        let root_name = root
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("workspace")
            .to_owned();

        // Writer task keeps stdin non-blocking for server-to-client replies.
        let writer_alive = Arc::clone(&alive);
        tokio::spawn(async move {
            while let Some(frame) = outgoing.recv().await {
                if stdin.write_all(&frame).await.is_err() || stdin.flush().await.is_err() {
                    break;
                }
            }
            writer_alive.store(false, Ordering::Relaxed);
        });

        // Drain stderr so the child never stalls on a full pipe.
        let server_label = binary.display().to_string();
        tokio::spawn(async move {
            let mut lines = BufReader::new(stderr).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                tracing::debug!(server = %server_label, "lsp stderr: {line}");
            }
        });

        // Reader task dispatches responses and answers server-to-client requests.
        let reader_pending = Arc::clone(&pending);
        let reader_alive = Arc::clone(&alive);
        let reader_writer = writer.clone();
        let reader_root_uri = root_uri.clone();
        let reader_root_name = root_name.clone();
        tokio::spawn(async move {
            let mut reader = BufReader::new(stdout);
            while let Ok(Some(message)) = read_frame(&mut reader).await {
                let id = message.get("id");
                let method = message.get("method").and_then(Value::as_str);

                match (id, method) {
                    // Server-to-client request: answer immediately so the server never deadlocks.
                    (Some(req_id), Some(method)) => {
                        let result = server_request_reply(
                            method,
                            message.get("params"),
                            &reader_root_uri,
                            &reader_root_name,
                        );
                        let reply = json!({
                            "jsonrpc": "2.0",
                            "id": req_id,
                            "result": result,
                        });
                        let _ = reader_writer.send(encode_frame(&reply));
                    }
                    // Response to one of our requests.
                    (Some(res_id), None) => {
                        if let Some(num) = res_id.as_i64()
                            && let Some(tx) = reader_pending
                                .lock()
                                .ok()
                                .and_then(|mut map| map.remove(&num))
                        {
                            let outcome = if let Some(err) = message.get("error") {
                                Err(RpcError {
                                    code: err.get("code").and_then(Value::as_i64).unwrap_or(-1),
                                    message: err
                                        .get("message")
                                        .and_then(Value::as_str)
                                        .unwrap_or("unknown error")
                                        .to_owned(),
                                })
                            } else {
                                Ok(message.get("result").cloned().unwrap_or(Value::Null))
                            };
                            let _ = tx.send(outcome);
                        }
                    }
                    // Notification from server: ignored for basic definition lookup.
                    _ => {}
                }
            }

            reader_alive.store(false, Ordering::Relaxed);
            if let Ok(mut map) = reader_pending.lock() {
                map.clear();
            }
        });

        let client = Arc::new(Self {
            root: root.to_path_buf(),
            writer,
            pending,
            next_id: AtomicI64::new(1),
            alive,
            open_docs: AsyncMutex::new(HashMap::new()),
            last_used: Mutex::new(Instant::now()),
            child: AsyncMutex::new(Some(child)),
        });

        let init_params = json!({
            "processId": std::process::id(),
            "rootUri": root_uri,
            "workspaceFolders": [{ "uri": root_uri, "name": root_name }],
            "capabilities": {
                "textDocument": {
                    "definition": { "linkSupport": true }
                },
                "workspace": {
                    "workspaceFolders": true
                }
            }
        });

        client
            .request("initialize", init_params, INIT_TIMEOUT)
            .await
            .context("initializing language server")?;
        client.notify("initialized", json!({}))?;

        Ok(client)
    }

    pub(crate) fn is_alive(&self) -> bool {
        self.alive.load(Ordering::Relaxed)
    }

    pub(crate) fn idle_for(&self) -> Duration {
        self.last_used
            .lock()
            .map(|at| at.elapsed())
            .unwrap_or_default()
    }

    pub(crate) fn touch(&self) {
        if let Ok(mut at) = self.last_used.lock() {
            *at = Instant::now();
        }
    }

    pub(crate) fn root(&self) -> &Path {
        &self.root
    }

    /// Sends `textDocument/didOpen` or `textDocument/didChange` when the file is new or changed.
    pub(crate) async fn sync_document(
        &self,
        path: &Path,
        language_id: &str,
        text: &str,
    ) -> Result<()> {
        self.touch();
        let uri = file_uri(path)?;
        let hash = *blake3::hash(text.as_bytes()).as_bytes();
        let mut docs = self.open_docs.lock().await;

        match docs.get_mut(path) {
            None => {
                self.notify(
                    "textDocument/didOpen",
                    json!({
                        "textDocument": {
                            "uri": uri,
                            "languageId": language_id,
                            "version": 1,
                            "text": text,
                        }
                    }),
                )?;
                docs.insert(path.to_path_buf(), OpenDoc { version: 1, hash });
            }
            Some(doc) if doc.hash != hash => {
                doc.version = doc.version.saturating_add(1);
                doc.hash = hash;
                self.notify(
                    "textDocument/didChange",
                    json!({
                        "textDocument": {
                            "uri": uri,
                            "version": doc.version,
                        },
                        "contentChanges": [{ "text": text }],
                    }),
                )?;
            }
            Some(_) => {}
        }

        Ok(())
    }

    /// Runs `textDocument/definition` at 0-based `line` and UTF-16 `column`.
    pub(crate) async fn definition(&self, path: &Path, line: u32, column: u32) -> Result<Value> {
        self.touch();
        let uri = file_uri(path)?;
        let params = json!({
            "textDocument": { "uri": uri },
            "position": { "line": line, "character": column },
        });
        self.request("textDocument/definition", params, REQUEST_TIMEOUT)
            .await
    }

    /// Best-effort graceful shutdown of the language server process.
    pub(crate) async fn shutdown(&self) {
        self.alive.store(false, Ordering::Relaxed);
        let _ = self
            .request("shutdown", Value::Null, Duration::from_secs(2))
            .await;
        let _ = self.notify("exit", Value::Null);

        if let Some(mut child) = self.child.lock().await.take() {
            let _ = child.kill().await;
            let _ = child.wait().await;
        }
    }

    fn notify(&self, method: &str, params: Value) -> Result<()> {
        if !self.is_alive() {
            bail!("language server is no longer running");
        }

        let payload = json!({
            "jsonrpc": "2.0",
            "method": method,
            "params": params,
        });
        self.writer
            .send(encode_frame(&payload))
            .map_err(|_| anyhow::anyhow!("language server closed stdin"))
    }

    async fn request(&self, method: &str, params: Value, limit: Duration) -> Result<Value> {
        if !self.is_alive() {
            bail!("language server is no longer running");
        }

        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let (tx, rx) = oneshot::channel();
        {
            let mut map = self
                .pending
                .lock()
                .map_err(|_| anyhow::anyhow!("LSP state poisoned"))?;
            map.insert(id, tx);
        }

        let payload = json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params,
        });

        if self.writer.send(encode_frame(&payload)).is_err() {
            if let Ok(mut map) = self.pending.lock() {
                map.remove(&id);
            }
            bail!("language server closed stdin");
        }

        match timeout(limit, rx).await {
            Ok(Ok(Ok(value))) => Ok(value),
            Ok(Ok(Err(rpc_err))) => Err(anyhow::Error::new(rpc_err)),
            Ok(Err(_)) => bail!("language server exited while answering {method}"),
            Err(_) => {
                if let Ok(mut map) = self.pending.lock() {
                    map.remove(&id);
                }
                bail!("language server timed out answering {method}");
            }
        }
    }
}

/// Builds a `PATH` for the spawned server that includes `binary`'s own folder,
/// the node's spawn path, and common toolchain folders under home.
fn lsp_path(binary: &Path) -> Option<std::ffi::OsString> {
    let mut dirs: Vec<PathBuf> = Vec::new();
    let mut push = |dir: PathBuf| {
        if !dirs.iter().any(|existing| existing == &dir) {
            dirs.push(dir);
        }
    };

    if let Some(dir) = binary.parent().filter(|p| !p.as_os_str().is_empty()) {
        push(dir.to_path_buf());
    }

    if let Some(search) = spawn_env::search_path() {
        for dir in env::split_paths(&search) {
            push(dir);
        }
    } else if let Some(path) = locate::path_with(&[binary]) {
        for dir in env::split_paths(&path) {
            push(dir);
        }
    }

    if let Some(home) = dirs::home_dir() {
        for rel in [".cargo/bin", "go/bin"] {
            let dir = home.join(rel);
            if dir.is_dir() {
                push(dir);
            }
        }
    }

    (!dirs.is_empty())
        .then(|| env::join_paths(dirs).ok())
        .flatten()
}
