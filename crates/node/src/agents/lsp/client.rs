//! One running language server child process speaking JSON-RPC LSP over stdio.

use crate::{
    AsyncMutex,
    files::paths,
    lock,
    machine::env, process::spawn_env,
    process::{GroupLeader, STOP_GRACE, command, own_group},
};
use anyhow::{Context, Result, bail};
use beep_proto::node::agent::LspLocation;
use serde_json::{Value, json};
use std::{
    collections::{HashMap, HashSet},
    path::{Component, Path, PathBuf},
    process::Stdio,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicI64, Ordering},
    },
    time::Duration,
};
use tokio::{
    io::{AsyncBufRead, AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader},
    sync::{mpsc, oneshot},
    task::spawn_blocking,
    time::{Instant, timeout},
};
use url::Url;

/// How long the initial `initialize` handshake may take.
const INIT_TIMEOUT: Duration = Duration::from_secs(15);
/// How long one `textDocument/definition` call may wait.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(12);
/// Largest JSON-RPC frame we accept from a language server.
const MAX_FRAME: usize = 16 * 1024 * 1024;
/// Largest single header line we accept before rejecting a malformed stream.
const MAX_HEADER_LINE: usize = 8 * 1024;
/// LSP error codes where retrying once after a brief pause often succeeds.
pub(super) const ERR_CONTENT_MODIFIED: i64 = -32801;
pub(super) const ERR_SERVER_CANCELLED: i64 = -32802;

/// A JSON-RPC error returned by a language server.
#[derive(Debug, Clone)]
pub(super) struct RpcError {
    pub(super) code: i64,
    pub(super) message: String,
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
pub(super) struct Client {
    root: PathBuf,
    writer: mpsc::UnboundedSender<Vec<u8>>,
    pending: PendingMap,
    next_id: AtomicI64,
    alive: Arc<AtomicBool>,
    open_docs: AsyncMutex<HashMap<PathBuf, OpenDoc>>,
    last_used: Mutex<Instant>,
    leader: AsyncMutex<Option<GroupLeader>>,
}

impl Client {
    /// Spawns `binary` with `args` in `root` and completes the LSP `initialize` handshake.
    pub(super) async fn spawn(root: &Path, binary: &Path, args: &[&str]) -> Result<Arc<Self>> {
        let mut cmd = command(binary);
        cmd.args(args)
            .current_dir(root)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        spawn_env::apply(&mut cmd, spawn_env::Scope::Probe);
        env::beside(&mut cmd, binary);
        own_group(&mut cmd);

        let mut child = cmd
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
        let leader = GroupLeader::new(child);

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
        let reader_label = binary.display().to_string();
        tokio::spawn(async move {
            let mut reader = BufReader::new(stdout);
            loop {
                match read_frame(&mut reader).await {
                    Ok(Some(message)) => dispatch_message(
                        message,
                        &reader_pending,
                        &reader_writer,
                        &reader_root_uri,
                        &reader_root_name,
                    ),
                    Ok(None) => break,
                    Err(err) => {
                        tracing::debug!(server = %reader_label, "lsp stdout ended: {err}");
                        break;
                    }
                }
            }

            reader_alive.store(false, Ordering::Relaxed);
            lock(&reader_pending).clear();
        });

        let client = Arc::new(Self {
            root: root.to_path_buf(),
            writer,
            pending,
            next_id: AtomicI64::new(1),
            alive,
            open_docs: AsyncMutex::new(HashMap::new()),
            last_used: Mutex::new(Instant::now()),
            leader: AsyncMutex::new(Some(leader)),
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

    pub(super) fn is_alive(&self) -> bool {
        self.alive.load(Ordering::Relaxed)
    }

    pub(super) fn idle_for(&self) -> Duration {
        lock(&self.last_used).elapsed()
    }

    pub(super) fn touch(&self) {
        *lock(&self.last_used) = Instant::now();
    }

    /// Sends `textDocument/didOpen` or `textDocument/didChange` when the file is new or changed.
    pub(super) async fn sync_document(
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
    pub(super) async fn definition(
        &self,
        path: &Path,
        line: u32,
        column: u32,
    ) -> Result<Vec<LspLocation>> {
        self.touch();
        let uri = file_uri(path)?;
        let params = json!({
            "textDocument": { "uri": uri },
            "position": { "line": line, "character": column },
        });
        let value = self
            .request("textDocument/definition", params, REQUEST_TIMEOUT)
            .await?;
        let root = self.root.clone();
        spawn_blocking(move || parse_locations(&value, &root))
            .await
            .context("parsing definition locations")
    }

    /// Graceful LSP `shutdown` + `exit`, then stops the process group.
    pub(super) async fn shutdown(&self) {
        if self.is_alive() {
            let _ = self
                .request("shutdown", Value::Null, Duration::from_secs(2))
                .await;
            let _ = self.notify("exit", Value::Null);
        }
        self.alive.store(false, Ordering::Relaxed);

        if let Some(mut leader) = self.leader.lock().await.take() {
            let _ = timeout(Duration::from_millis(500), leader.child_mut().wait()).await;
            leader.stop(STOP_GRACE).await;
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
        lock(&self.pending).insert(id, tx);

        let payload = json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params,
        });

        if self.writer.send(encode_frame(&payload)).is_err() {
            lock(&self.pending).remove(&id);
            bail!("language server closed stdin");
        }

        match timeout(limit, rx).await {
            Ok(Ok(Ok(value))) => Ok(value),
            Ok(Ok(Err(rpc_err))) => Err(anyhow::Error::new(rpc_err)),
            Ok(Err(_)) => bail!("language server exited while answering {method}"),
            Err(_) => {
                lock(&self.pending).remove(&id);
                bail!("language server timed out answering {method}");
            }
        }
    }
}

fn dispatch_message(
    message: Value,
    pending: &PendingMap,
    writer: &mpsc::UnboundedSender<Vec<u8>>,
    root_uri: &str,
    root_name: &str,
) {
    let id = message.get("id").filter(|v| !v.is_null());
    let method = message.get("method").and_then(Value::as_str);

    match (id, method) {
        (Some(req_id), Some(method)) => {
            let result = server_request_reply(method, message.get("params"), root_uri, root_name);
            let reply = json!({ "jsonrpc": "2.0", "id": req_id, "result": result });
            let _ = writer.send(encode_frame(&reply));
        }
        (Some(res_id), None) => {
            if let Some(num) = res_id.as_i64()
                && let Some(tx) = lock(pending).remove(&num)
            {
                let outcome = match message.get("error") {
                    Some(err) => Err(RpcError {
                        code: err.get("code").and_then(Value::as_i64).unwrap_or(-1),
                        message: err
                            .get("message")
                            .and_then(Value::as_str)
                            .unwrap_or("unknown error")
                            .to_owned(),
                    }),
                    None => Ok(message.get("result").cloned().unwrap_or(Value::Null)),
                };
                let _ = tx.send(outcome);
            }
        }
        _ => {}
    }
}

/// Encodes a JSON-RPC message with an LSP `Content-Length` header.
pub(super) fn encode_frame(message: &Value) -> Vec<u8> {
    let body = serde_json::to_vec(message).unwrap_or_default();
    let mut frame = format!("Content-Length: {}\r\n\r\n", body.len()).into_bytes();
    frame.extend_from_slice(&body);
    frame
}

/// Reads the next JSON-RPC frame from an LSP stdout stream; `Ok(None)` on clean EOF.
pub(super) async fn read_frame<R: AsyncBufRead + Unpin>(reader: &mut R) -> Result<Option<Value>> {
    let mut content_length: Option<usize> = None;
    let mut saw_header = false;
    let mut line = String::new();

    loop {
        line.clear();
        let read = reader
            .take((MAX_HEADER_LINE + 1) as u64)
            .read_line(&mut line)
            .await
            .context("reading LSP header")?;

        if read == 0 {
            if saw_header {
                bail!("unexpected EOF in LSP headers");
            }
            return Ok(None);
        }

        if read > MAX_HEADER_LINE {
            bail!("LSP header line exceeded limit");
        }

        let trimmed = line.trim_end_matches(['\r', '\n']);

        if trimmed.is_empty() {
            if saw_header {
                break;
            }
            continue;
        }

        saw_header = true;

        if let Some((name, value)) = trimmed.split_once(':')
            && name.trim().eq_ignore_ascii_case("content-length")
        {
            content_length = Some(
                value
                    .trim()
                    .parse()
                    .context("invalid LSP Content-Length header")?,
            );
        }
    }

    let length = content_length.context("missing LSP Content-Length header")?;

    if length > MAX_FRAME {
        bail!("LSP frame of {length} bytes exceeds limit");
    }

    let mut body = vec![0u8; length];
    reader
        .read_exact(&mut body)
        .await
        .context("reading LSP body")?;
    serde_json::from_slice(&body).context("parsing LSP JSON body")
}

/// Converts an absolute filesystem path into a `file://` URI string.
pub(super) fn file_uri(path: &Path) -> Result<String> {
    Url::from_file_path(path)
        .map(Into::into)
        .map_err(|()| anyhow::anyhow!("cannot convert {} to a file URI", path.display()))
}

/// Replies to common server-to-client LSP requests so servers never block waiting on us.
pub(super) fn server_request_reply(
    method: &str,
    params: Option<&Value>,
    root_uri: &str,
    root_name: &str,
) -> Value {
    match method {
        "workspace/configuration" => {
            let count = params
                .and_then(|value| value.get("items"))
                .and_then(Value::as_array)
                .map_or(0, Vec::len);
            Value::Array(vec![Value::Null; count])
        }
        "workspace/workspaceFolders" => json!([{ "uri": root_uri, "name": root_name }]),
        _ => Value::Null,
    }
}

/// Parses a `textDocument/definition` response (`null`, `Location`, `Location[]`, or
/// `LocationLink[]`) into deduplicated workspace-normalized [`LspLocation`] values.
pub(super) fn parse_locations(result: &Value, root: &Path) -> Vec<LspLocation> {
    let items: &[Value] = match result {
        Value::Array(array) => array,
        Value::Object(_) => std::slice::from_ref(result),
        _ => return Vec::new(),
    };

    let mut out = Vec::with_capacity(items.len());
    let mut seen: HashSet<(String, bool, u32, u32)> = HashSet::new();

    for item in items {
        let Some(location) = parse_one_location(item, root) else {
            continue;
        };
        let key = (
            location.path.clone(),
            location.inside_root,
            location.line,
            location.column,
        );

        if seen.insert(key) {
            out.push(location);
        }
    }

    out
}

fn parse_one_location(item: &Value, root: &Path) -> Option<LspLocation> {
    let (uri, range) = if let Some(target_uri) = item.get("targetUri").and_then(Value::as_str) {
        let range = item
            .get("targetSelectionRange")
            .or_else(|| item.get("targetRange"))?;
        (target_uri, range)
    } else {
        (item.get("uri").and_then(Value::as_str)?, item.get("range")?)
    };

    let start = range.get("start")?;
    let line = u32::try_from(start.get("line")?.as_u64()?).ok()?;
    let column = u32::try_from(start.get("character")?.as_u64()?).ok()?;

    let url = Url::parse(uri).ok()?;
    if url.scheme() != "file" {
        return None;
    }

    let raw_path = url.to_file_path().ok()?;
    let resolved = paths::canonicalize(&raw_path).unwrap_or(raw_path);
    let (path, inside_root) = match resolved.strip_prefix(root) {
        Ok(rel) => (rel_to_slash(rel)?, true),
        Err(_) => (resolved.to_string_lossy().into_owned(), false),
    };

    Some(LspLocation {
        path,
        inside_root,
        line,
        column,
    })
}

fn rel_to_slash(rel: &Path) -> Option<String> {
    let mut parts = Vec::new();

    for component in rel.components() {
        match component {
            Component::Normal(part) => parts.push(part.to_str()?),
            Component::CurDir => {}
            _ => return None,
        }
    }

    (!parts.is_empty()).then(|| parts.join("/"))
}
