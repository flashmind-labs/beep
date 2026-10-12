//! A folder per user that the agent downloads files into and runs Python or
//! shell code on. Code runs under Drop (<https://droprun.sh>) with no network,
//! read-write access to that folder only, and limits on time, memory and file
//! size. Files older than a day are swept.

use crate::{AsyncMutex, user::User};
use anyhow::{Context, Result, anyhow, bail, ensure};
use async_trait::async_trait;
use flashmind::types::{
    ToolRegistry,
    tool::{Tool, ToolContext, ToolResult},
};
use futures::StreamExt;
use reqwest::{
    Client, Response, StatusCode, Url,
    header::{CONTENT_TYPE, LOCATION},
    redirect::Policy,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    env,
    io::ErrorKind,
    net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr},
    path::{Component, Path, PathBuf},
    process::Stdio,
    sync::Arc,
    time::{Duration, SystemTime},
};
use tokio::{
    fs::{self, File, OpenOptions},
    io::{self, AsyncRead, AsyncReadExt, AsyncWriteExt},
    net::lookup_host,
    process::Command,
};

/// Largest file `download_file` saves.
pub const MAX_DOWNLOAD: u64 = 20 * 1024 * 1024;
/// Largest a user's folder may grow through downloads.
const MAX_FOLDER: u64 = 200 * 1024 * 1024;
/// Files older than this are deleted by the sweep.
const FILE_TTL: Duration = Duration::from_secs(24 * 60 * 60);
const SWEEP_EVERY: Duration = Duration::from_secs(60 * 60);
/// Wall time a run gets inside the sandbox.
const RUN_SECS: u64 = 60;
const MAX_MEMORY: u64 = 4 * 1024 * 1024 * 1024;
/// Largest file code in the sandbox may write.
const MAX_WRITE: u64 = 50 * 1024 * 1024;
/// Bytes of stdout and of stderr passed back to the model.
const MAX_OUTPUT: usize = 16 * 1024;
const MAX_LISTED: usize = 100;
const MAX_REDIRECTS: usize = 5;

/// Drop's shared config. No host files, no network, and only the variables
/// code needs, so nothing from the server's environment leaks in.
const BASE_CONFIG: &str = r#"runtime = "native"
mounts = []
blocked_paths = []

[environ]
exposed_vars = ["PATH", "LANG", "LC_*"]
set_vars = ["MPLBACKEND=Agg", "PYTHONDONTWRITEBYTECODE=1"]

[net]
mode = "off"
"#;

pub struct Sandbox {
    root: PathBuf,
    drop_config: PathBuf,
    /// Serializes `drop init`, which writes into the shared config folder.
    init: AsyncMutex<()>,
}

#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "lowercase")]
enum Language {
    Python,
    Bash,
}

impl Language {
    fn command(self) -> &'static [&'static str] {
        match self {
            Self::Python => &["python3", "-"],
            Self::Bash => &["bash", "-s"],
        }
    }
}

#[derive(Serialize)]
struct RunOutput {
    exit_code: Option<i32>,
    timed_out: bool,
    stdout: String,
    stderr: String,
    files: Vec<FileEntry>,
}

#[derive(Serialize)]
struct FileEntry {
    path: String,
    bytes: u64,
    /// Written during this run.
    changed: bool,
}

#[derive(Serialize)]
struct Saved {
    path: String,
    bytes: u64,
    content_type: Option<String>,
}

impl Sandbox {
    /// Prepares `root` and Drop's shared config. Fails when Drop is missing,
    /// so a server without it does not offer tools that cannot work.
    pub async fn new(root: PathBuf) -> Result<Arc<Self>> {
        let version = Command::new("drop")
            .arg("--version")
            .output()
            .await
            .context("running drop; is it installed?")?;
        ensure!(version.status.success(), "drop --version failed");
        fs::create_dir_all(&root).await?;
        let home = env::var_os("HOME").context("HOME is not set")?;
        let drop_config = PathBuf::from(home).join(".config/drop");
        fs::create_dir_all(&drop_config).await?;
        fs::write(drop_config.join("base.toml"), BASE_CONFIG).await?;
        log::info!(
            "sandbox ready; root={} drop={}",
            root.display(),
            String::from_utf8_lossy(&version.stdout).trim()
        );
        Ok(Arc::new(Self {
            root,
            drop_config,
            init: AsyncMutex::new(()),
        }))
    }

    /// The user's folder, with its Drop environment created on first use.
    async fn folder(&self, user_id: &str) -> Result<PathBuf> {
        ensure!(
            !user_id.is_empty() && user_id.chars().all(|c| c.is_ascii_alphanumeric()),
            "unexpected user id"
        );
        let dir = self.root.join(user_id);
        fs::create_dir_all(&dir).await?;
        let _guard = self.init.lock().await;
        if !fs::try_exists(self.drop_config.join(format!("{user_id}.toml"))).await? {
            let out = Command::new("drop")
                .args(["init", user_id])
                .current_dir(&dir)
                .stdin(Stdio::null())
                .output()
                .await?;
            ensure!(
                out.status.success(),
                "drop init failed: {}",
                String::from_utf8_lossy(&out.stderr).trim()
            );
        }
        Ok(dir)
    }

    async fn run(&self, user_id: &str, language: Language, code: &str) -> Result<RunOutput> {
        let dir = self.folder(user_id).await?;
        let started = SystemTime::now();
        let mut child = Command::new("drop")
            .args(["run", "-e", user_id, "--", "prlimit"])
            .arg(format!("--as={MAX_MEMORY}"))
            .arg(format!("--fsize={MAX_WRITE}"))
            .args(["--", "timeout", "-s", "KILL"])
            .arg(RUN_SECS.to_string())
            .args(language.command())
            .current_dir(&dir)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            // Killing drop takes everything in the sandbox with it.
            .kill_on_drop(true)
            .spawn()
            .context("starting drop")?;
        let mut stdin = child.stdin.take().context("no stdin")?;
        let stdout = child.stdout.take().context("no stdout")?;
        let stderr = child.stderr.take().context("no stderr")?;
        // The code is written while the output is read, so a script that
        // prints before reading all of it cannot block both sides. A write
        // that fails because the code exited early still leaves its output.
        let feed = async move {
            let _ = stdin.write_all(code.as_bytes()).await;
        };
        // `timeout` inside the sandbox ends the run; this only catches a
        // sandbox that hangs on its own.
        let (_, stdout, stderr, status) =
            tokio::time::timeout(Duration::from_secs(RUN_SECS + 15), async {
                tokio::join!(feed, read_head(stdout), read_head(stderr), child.wait())
            })
            .await
            .map_err(|_| anyhow!("the sandbox did not finish"))?;
        let exit_code = status?.code();
        Ok(RunOutput {
            exit_code,
            // timeout's SIGKILL shows as 128 + 9.
            timed_out: exit_code == Some(137),
            stdout: clip(&stdout?),
            stderr: clip(&stderr?),
            files: list_files(&dir, started).await?,
        })
    }

    async fn download(&self, user_id: &str, url: &str, name: Option<&str>) -> Result<Saved> {
        let dir = self.folder(user_id).await?;
        let Followed { response, url } = follow(url).await?;
        let used = folder_size(&dir).await?;
        let room = MAX_FOLDER.saturating_sub(used).min(MAX_DOWNLOAD);
        let content_type = content_type(&response);
        let file_name = file_name(name, &url);
        let path = dir.join(&file_name);
        let partial = dir.join(format!(".{file_name}.part"));
        let mut file = create_fresh(&partial).await?;
        let mut bytes = 0;
        let mut body = response.bytes_stream();
        while let Some(chunk) = body.next().await {
            let chunk = chunk.context("the download broke off")?;
            bytes += chunk.len() as u64;
            if bytes > room {
                drop(file);
                fs::remove_file(&partial).await.ok();
                if room < MAX_DOWNLOAD {
                    bail!("the user's folder is full; delete files first");
                }
                bail!("the file is larger than {} MiB", MAX_DOWNLOAD >> 20);
            }
            file.write_all(&chunk).await?;
        }
        file.flush().await?;
        fs::rename(&partial, &path).await?;
        Ok(Saved {
            path: file_name,
            bytes,
            content_type,
        })
    }

    /// The file at `path` in the user's folder, with its plain name.
    pub(crate) async fn read(&self, user_id: &str, path: &str) -> Result<FolderFile> {
        let dir = self.folder(user_id).await?;
        read_in(&dir, path).await
    }

    /// Deletes files older than `FILE_TTL` every hour.
    pub fn start_sweep(self: &Arc<Self>) {
        let sandbox = Arc::downgrade(self);
        tokio::spawn(async move {
            let mut tick = tokio::time::interval(SWEEP_EVERY);
            loop {
                tick.tick().await;
                let Some(sandbox) = sandbox.upgrade() else {
                    return;
                };
                match sweep(&sandbox.root, SystemTime::now() - FILE_TTL).await {
                    Ok(0) => {}
                    Ok(n) => log::info!("sandbox sweep removed {n} files"),
                    Err(e) => log::warn!("sandbox sweep failed: {e:#}"),
                }
            }
        });
    }
}

pub fn register(registry: &mut ToolRegistry, sandbox: &Arc<Sandbox>, user: &Arc<User>) {
    registry.register(Arc::new(ExecuteCodeTool {
        sandbox: sandbox.clone(),
        user: user.clone(),
    }));
    registry.register(Arc::new(DownloadFileTool {
        sandbox: sandbox.clone(),
        user: user.clone(),
    }));
}

struct ExecuteCodeTool {
    sandbox: Arc<Sandbox>,
    user: Arc<User>,
}

#[async_trait]
impl Tool for ExecuteCodeTool {
    fn name(&self) -> &str {
        "execute_code"
    }

    // Runs share the user's sandbox folder.
    fn concurrent(&self) -> bool {
        false
    }
    fn description(&self) -> &str {
        "Run Python or bash in a sandbox for calculations, data analysis, reading PDFs and converting files. The working directory is the user's file folder, where download_file saves files; files there are deleted after 24 hours. There is no network access. Python has numpy, pandas, scipy, matplotlib (save charts to files), openpyxl, pypdf and pdfplumber; pdftotext and pdfinfo are installed. Runs stop after 60 seconds. Print what you need to see; long output is cut."
    }
    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "language": { "type": "string", "enum": ["python", "bash"] },
                "code": { "type": "string", "description": "The program, passed on stdin" }
            },
            "required": ["language", "code"],
            "additionalProperties": false
        })
    }
    fn timeout_secs(&self) -> Option<u64> {
        Some(RUN_SECS + 30)
    }
    fn humanize(&self, _args: &Value) -> String {
        "Running code…".into()
    }
    async fn execute(&self, ctx: ToolContext<'_>) -> anyhow::Result<ToolResult> {
        #[derive(Deserialize)]
        struct Args {
            language: Language,
            code: String,
        }
        let args: Args = match serde_json::from_value(ctx.args.clone()) {
            Ok(args) => args,
            Err(e) => return Ok(ToolResult::failure(ctx.tool_call_id, e.to_string())),
        };
        match self
            .sandbox
            .run(self.user.external_id(), args.language, &args.code)
            .await
        {
            Ok(out) => Ok(ToolResult::success(
                ctx.tool_call_id,
                serde_json::to_string(&out)?,
            )),
            Err(e) => {
                log::warn!("sandbox run failed: {e:#}");
                Ok(ToolResult::failure(ctx.tool_call_id, format!("{e:#}")))
            }
        }
    }
}

struct DownloadFileTool {
    sandbox: Arc<Sandbox>,
    user: Arc<User>,
}

#[async_trait]
impl Tool for DownloadFileTool {
    fn name(&self) -> &str {
        "download_file"
    }

    // Runs share the user's sandbox folder.
    fn concurrent(&self) -> bool {
        false
    }
    fn description(&self) -> &str {
        "Download a file such as a PDF or spreadsheet from a public http(s) URL into the user's file folder, so execute_code can read it. Files up to 20 MiB; they are deleted after 24 hours. Saving under an existing name replaces that file."
    }
    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "url": { "type": "string" },
                "filename": { "type": "string", "description": "Name to save as; taken from the URL when left out" }
            },
            "required": ["url"],
            "additionalProperties": false
        })
    }
    fn timeout_secs(&self) -> Option<u64> {
        Some(120)
    }
    fn humanize(&self, _args: &Value) -> String {
        "Downloading file…".into()
    }
    async fn execute(&self, ctx: ToolContext<'_>) -> anyhow::Result<ToolResult> {
        let url = ctx.args.get("url").and_then(Value::as_str).unwrap_or("");
        let name = ctx.args.get("filename").and_then(Value::as_str);
        match self
            .sandbox
            .download(self.user.external_id(), url, name)
            .await
        {
            Ok(saved) => Ok(ToolResult::success(
                ctx.tool_call_id,
                serde_json::to_string(&saved)?,
            )),
            Err(e) => Ok(ToolResult::failure(
                ctx.tool_call_id,
                format!("Could not download {url}: {e:#}"),
            )),
        }
    }
}

/// The file at `path` under `dir`. A link as the file itself is refused, and
/// the path is resolved in full, so a linked folder on the way cannot lead
/// out of `dir`.
async fn read_in(dir: &Path, path: &str) -> Result<FolderFile> {
    let relative = Path::new(path.trim_start_matches("./"));
    ensure!(
        relative
            .components()
            .all(|part| matches!(part, Component::Normal(_))),
        "the path must be inside the user's folder"
    );
    let full = dir.join(relative);
    let meta = fs::symlink_metadata(&full)
        .await
        .context("there is no such file")?;
    ensure!(meta.is_file(), "it is not a file");
    let real = fs::canonicalize(&full)
        .await
        .context("there is no such file")?;
    ensure!(
        real.starts_with(fs::canonicalize(dir).await?),
        "the path must be inside the user's folder"
    );
    // Read through the resolved path, so a link swapped in for the file
    // after the checks is not followed.
    let mut file = File::open(&real).await?;
    let meta = file.metadata().await?;
    ensure!(meta.is_file(), "it is not a file");
    ensure!(
        meta.len() <= MAX_DOWNLOAD,
        "the file is larger than {} MiB",
        MAX_DOWNLOAD >> 20
    );
    let mut data = Vec::new();
    file.read_to_end(&mut data).await?;
    let name = relative
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("file")
        .to_owned();
    Ok(FolderFile { name, data })
}

/// Opens `path` as a new file, removing what was there first, so a link
/// planted under that name is replaced rather than written through.
async fn create_fresh(path: &Path) -> Result<File> {
    match fs::remove_file(path).await {
        Err(e) if e.kind() != ErrorKind::NotFound => return Err(e.into()),
        _ => {}
    }
    Ok(OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .await?)
}

/// A file read from the user's folder.
pub(crate) struct FolderFile {
    pub(crate) name: String,
    pub(crate) data: Vec<u8>,
}

/// The final answer to a request after its redirects, and where it came from.
pub(crate) struct Followed {
    pub(crate) response: Response,
    url: Url,
}

/// Fetches `url`, and checks the answer is a file of at most `MAX_DOWNLOAD`.
async fn follow(url: &str) -> Result<Followed> {
    let followed = follow_public(url).await?;
    let response = &followed.response;
    if response.status() != StatusCode::OK {
        bail!("the server answered {}", response.status());
    }
    if response.content_length().is_some_and(|n| n > MAX_DOWNLOAD) {
        bail!("the file is larger than {} MiB", MAX_DOWNLOAD >> 20);
    }
    Ok(followed)
}

/// Fetches `url`, following up to `MAX_REDIRECTS` redirects through public
/// addresses only. The body is left unread.
pub(crate) async fn follow_public(url: &str) -> Result<Followed> {
    let mut url = Url::parse(url).context("not a valid URL")?;
    let mut response = None;
    for _ in 0..=MAX_REDIRECTS {
        let reply = fetch(&url).await?;
        if !reply.status().is_redirection() {
            response = Some(reply);
            break;
        }
        let location = reply
            .headers()
            .get(LOCATION)
            .and_then(|v| v.to_str().ok())
            .context("redirect without a location")?;
        url = url.join(location).context("bad redirect")?;
    }
    let response = response.context("too many redirects")?;
    Ok(Followed { response, url })
}

/// The response's `Content-Type`, when it is readable text.
pub(crate) fn content_type(response: &Response) -> Option<String> {
    response
        .headers()
        .get(CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .map(str::to_owned)
}

/// A file fetched from a public URL.
pub(crate) struct Fetched {
    /// The URL's last path segment, cleaned up.
    pub(crate) name: String,
    pub(crate) content_type: Option<String>,
    pub(crate) data: Vec<u8>,
}

/// Fetches a file of at most `MAX_DOWNLOAD` from a public http(s) URL, with
/// the same address checks as `download_file`.
pub(crate) async fn fetch_public(url: &str) -> Result<Fetched> {
    let Followed { response, url } = follow(url).await?;
    let content_type = content_type(&response);
    let mut data = Vec::new();
    let mut body = response.bytes_stream();
    while let Some(chunk) = body.next().await {
        let chunk = chunk.context("the download broke off")?;
        ensure!(
            (data.len() + chunk.len()) as u64 <= MAX_DOWNLOAD,
            "the file is larger than {} MiB",
            MAX_DOWNLOAD >> 20
        );
        data.extend_from_slice(&chunk);
    }
    Ok(Fetched {
        name: file_name(None, &url),
        content_type,
        data,
    })
}

/// One request to `url`, without following redirects, after checking that
/// every address the host resolves to is public. The connection is pinned to
/// the checked address so a second lookup cannot swap in another.
async fn fetch(url: &Url) -> Result<Response> {
    let Resolved { domain, addrs } = public_addresses(url).await?;
    let mut client = Client::builder().redirect(Policy::none());
    if let Some(domain) = domain {
        client = client.resolve(domain, addrs[0]);
    }
    let client = client
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(90))
        .user_agent("Beep")
        .build()?;
    Ok(client.get(url.clone()).send().await?)
}

/// Where a URL points: its domain name, if it has one rather than an
/// address, and the addresses to connect to. `addrs` is never empty.
#[derive(Debug)]
struct Resolved<'a> {
    domain: Option<&'a str>,
    addrs: Vec<SocketAddr>,
}

/// Where `url` points, checked to be public addresses only.
async fn public_addresses(url: &Url) -> Result<Resolved<'_>> {
    ensure!(
        matches!(url.scheme(), "http" | "https"),
        "only http and https URLs are allowed"
    );
    ensure!(url.as_str().len() <= 2048, "the URL is too long");
    let host = url.host_str().context("the URL has no host")?;
    let port = url.port_or_known_default().context("the URL has no port")?;
    // An IPv6 address comes in brackets, which a lookup would not take.
    let literal = host
        .strip_prefix('[')
        .and_then(|h| h.strip_suffix(']'))
        .unwrap_or(host);
    let (domain, addrs) = match literal.parse::<IpAddr>() {
        Ok(ip) => (None, vec![SocketAddr::new(ip, port)]),
        Err(_) => (
            Some(host),
            lookup_host((host, port))
                .await
                .context("the host does not resolve")?
                .collect(),
        ),
    };
    ensure!(!addrs.is_empty(), "the host does not resolve");
    ensure!(
        addrs.iter().all(|a| is_public(a.ip())),
        "the host is not on the public internet"
    );
    Ok(Resolved { domain, addrs })
}

fn is_public(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ip) => is_public_v4(ip),
        IpAddr::V6(ip) => match ip.to_ipv4_mapped() {
            Some(v4) => is_public_v4(v4),
            None => is_public_v6(ip),
        },
    }
}

fn is_public_v4(ip: Ipv4Addr) -> bool {
    let [a, b, c, _] = ip.octets();
    !(ip.is_private()
        || ip.is_loopback()
        || ip.is_link_local()
        || ip.is_broadcast()
        || ip.is_documentation()
        || ip.is_unspecified()
        || ip.is_multicast()
        || a == 0
        || a >= 240
        || (a == 100 && (64..128).contains(&b))
        || (a == 192 && b == 0 && c == 0)
        || (a == 198 && (18..20).contains(&b)))
}

/// Global unicast only (2000::/3), which also keeps out mesh overlays that
/// live in 200::/7, unique-local and link-local addresses.
fn is_public_v6(ip: Ipv6Addr) -> bool {
    let first = ip.segments()[0];
    first & 0xe000 == 0x2000 && first != 0x2001 || {
        // 2001::/16 holds both public space and documentation/Teredo ranges.
        first == 0x2001 && !matches!(ip.segments()[1], 0x0000 | 0x0db8)
    }
}

/// A plain file name from `name` or the URL's last path segment.
fn file_name(name: Option<&str>, url: &Url) -> String {
    let raw = name
        .filter(|n| !n.trim().is_empty())
        .map(str::to_owned)
        .or_else(|| {
            url.path_segments()
                .and_then(|mut s| s.next_back().map(str::to_owned))
        })
        .unwrap_or_default();
    let cleaned: String = raw
        .rsplit('/')
        .next()
        .unwrap_or("")
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_') {
                c
            } else {
                '_'
            }
        })
        .take(100)
        .collect();
    let cleaned = cleaned.trim_start_matches('.');
    if cleaned.is_empty() {
        "download".into()
    } else {
        cleaned.to_owned()
    }
}

/// The start of what `pipe` gives, enough for `clip`. The rest is read and
/// dropped, so a program that prints without end neither blocks nor fills
/// the server's memory.
async fn read_head(mut pipe: impl AsyncRead + Unpin) -> io::Result<Vec<u8>> {
    let mut head = Vec::new();
    // A few bytes past the limit finish the character `clip` cuts at.
    (&mut pipe)
        .take(MAX_OUTPUT as u64 + 4)
        .read_to_end(&mut head)
        .await?;
    io::copy(&mut pipe, &mut io::sink()).await?;
    Ok(head)
}

fn clip(bytes: &[u8]) -> String {
    let text = String::from_utf8_lossy(bytes);
    if text.len() <= MAX_OUTPUT {
        return text.into_owned();
    }
    let end = text.floor_char_boundary(MAX_OUTPUT);
    format!("{}\n[cut at {MAX_OUTPUT} bytes]", &text[..end])
}

/// Every file under `dir`, recursively.
async fn walk(dir: &Path) -> Result<Vec<(PathBuf, std::fs::Metadata)>> {
    let mut files = Vec::new();
    let mut pending = vec![dir.to_path_buf()];
    while let Some(next) = pending.pop() {
        let mut entries = fs::read_dir(&next).await?;
        while let Some(entry) = entries.next_entry().await? {
            let meta = entry.metadata().await?;
            if meta.is_dir() {
                pending.push(entry.path());
            } else if meta.is_file() {
                files.push((entry.path(), meta));
            }
        }
    }
    Ok(files)
}

async fn list_files(dir: &Path, since: SystemTime) -> Result<Vec<FileEntry>> {
    let mut files: Vec<FileEntry> = walk(dir)
        .await?
        .into_iter()
        .filter_map(|(path, meta)| {
            let rel = path.strip_prefix(dir).ok()?.to_string_lossy().into_owned();
            Some(FileEntry {
                changed: meta.modified().is_ok_and(|m| m >= since),
                bytes: meta.len(),
                path: rel,
            })
        })
        .collect();
    files.sort_by(|a, b| b.changed.cmp(&a.changed).then(a.path.cmp(&b.path)));
    files.truncate(MAX_LISTED);
    Ok(files)
}

async fn folder_size(dir: &Path) -> Result<u64> {
    Ok(walk(dir).await?.iter().map(|(_, meta)| meta.len()).sum())
}

/// Removes files under `root` last modified before `cutoff`.
async fn sweep(root: &Path, cutoff: SystemTime) -> Result<usize> {
    let mut removed = 0;
    for (path, meta) in walk(root).await? {
        if meta.modified().is_ok_and(|m| m < cutoff) && fs::remove_file(&path).await.is_ok() {
            removed += 1;
        }
    }
    Ok(removed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_public_addresses_pass() {
        for ip in [
            "127.0.0.1",
            "10.0.0.2",
            "172.17.0.1",
            "192.168.1.1",
            "169.254.169.254",
            "100.64.0.1",
            "0.0.0.0",
            "::1",
            "fe80::1",
            "fd00::1",
            "290:78f5::1",
            "2001:db8::1",
            "::ffff:127.0.0.1",
        ] {
            assert!(!is_public(ip.parse().expect("valid ip")), "{ip}");
        }
        for ip in ["93.184.215.14", "2606:4700::1111", "2a01:4f8::1"] {
            assert!(is_public(ip.parse().expect("valid ip")), "{ip}");
        }
    }

    #[tokio::test]
    async fn refuses_private_hosts_without_connecting() {
        for url in [
            "http://127.0.0.1/a.pdf",
            "http://localhost:8080/",
            "file:///etc/passwd",
            "http://[::1]/",
        ] {
            assert!(
                fetch(&Url::parse(url).expect("valid url")).await.is_err(),
                "{url}"
            );
        }
    }

    #[tokio::test]
    async fn ipv6_literals_are_checked_without_a_lookup() {
        let url = Url::parse("https://[2606:4700::1111]/a.pdf").expect("valid url");
        let resolved = public_addresses(&url).await.expect("a public address");
        assert_eq!(resolved.domain, None);
        assert_eq!(
            resolved.addrs,
            [SocketAddr::new(
                "2606:4700::1111".parse().expect("valid ip"),
                443
            )]
        );
        let private = Url::parse("http://[fd00::1]/").expect("valid url");
        assert!(public_addresses(&private).await.is_err());
    }

    #[tokio::test]
    async fn reads_stay_in_the_folder() {
        let dir = tempfile::tempdir().expect("temp dir");
        let user = dir.path().join("abc");
        let outside = dir.path().join("outside");
        std::fs::create_dir_all(user.join("sub")).expect("create folder");
        std::fs::create_dir_all(&outside).expect("create folder");
        std::fs::write(user.join("sub/ok.txt"), "ok").expect("write");
        std::fs::write(outside.join("secret.txt"), "secret").expect("write");
        std::os::unix::fs::symlink(&outside, user.join("root")).expect("link");
        std::os::unix::fs::symlink(user.join("sub/ok.txt"), user.join("link.txt")).expect("link");
        let file = read_in(&user, "./sub/ok.txt").await.expect("read");
        assert_eq!(
            (file.name.as_str(), file.data.as_slice()),
            ("ok.txt", &b"ok"[..])
        );
        for path in [
            "root/secret.txt",
            "link.txt",
            "../outside/secret.txt",
            "/etc/hosts",
        ] {
            assert!(read_in(&user, path).await.is_err(), "{path}");
        }
    }

    #[tokio::test]
    async fn a_planted_partial_link_is_not_written_through() {
        let dir = tempfile::tempdir().expect("temp dir");
        let target = dir.path().join("target");
        let partial = dir.path().join(".a.pdf.part");
        std::fs::write(&target, "keep").expect("write");
        std::os::unix::fs::symlink(&target, &partial).expect("link");
        let mut file = create_fresh(&partial).await.expect("open");
        file.write_all(b"new").await.expect("write");
        file.flush().await.expect("flush");
        assert_eq!(std::fs::read(&target).expect("read"), b"keep");
        assert_eq!(std::fs::read(&partial).expect("read"), b"new");
    }

    #[test]
    fn file_names_stay_in_the_folder() {
        let url = Url::parse("https://example.com/docs/report%20v2.pdf").expect("valid url");
        assert_eq!(file_name(None, &url), "report_20v2.pdf");
        assert_eq!(file_name(Some("../../etc/passwd"), &url), "passwd");
        assert_eq!(file_name(Some(".."), &url), "download");
        assert_eq!(file_name(Some(".hidden"), &url), "hidden");
        let bare = Url::parse("https://example.com/").expect("valid url");
        assert_eq!(file_name(None, &bare), "download");
    }

    #[test]
    fn output_is_clipped_on_a_char_boundary() {
        let long = "€".repeat(MAX_OUTPUT);
        let clipped = clip(long.as_bytes());
        assert!(clipped.ends_with(&format!("[cut at {MAX_OUTPUT} bytes]")));
    }

    #[tokio::test]
    async fn only_the_head_of_endless_output_is_kept() {
        let endless = tokio::io::repeat(b'y').take(10 * MAX_OUTPUT as u64);
        let head = read_head(endless).await.unwrap();
        assert_eq!(head.len(), MAX_OUTPUT + 4);
    }

    #[tokio::test]
    async fn sweep_removes_only_old_files() {
        let dir = tempfile::tempdir().expect("temp dir");
        let user = dir.path().join("abc");
        std::fs::create_dir_all(&user).expect("create folder");
        std::fs::write(user.join("old.pdf"), "x").expect("write");
        let cutoff = SystemTime::now() + Duration::from_secs(1);
        std::thread::sleep(Duration::from_millis(1100));
        std::fs::write(user.join("new.pdf"), "x").expect("write");
        assert_eq!(sweep(dir.path(), cutoff).await.expect("sweep"), 1);
        assert!(user.join("new.pdf").exists());
        assert!(!user.join("old.pdf").exists());
    }
}
