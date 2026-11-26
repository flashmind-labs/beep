//! The Artifacts tab of a coding agent session: a page or document the
//! agent made, read from its machine through the Files tab's read path and
//! made ready to show. Markdown becomes the bar's cleaned HTML; SVG and
//! images come back as bytes for an `<img>`; an HTML page is served from
//! its own scheme, `beepview:`, so it never runs in the window's origin.
//!
//! The frame it shows in is sandboxed without `allow-same-origin`, and
//! Tauri gives its IPC scripts and invoke key to the main frame only, so
//! the page cannot call Beep. The page's own response carries a CSP with
//! the same sandbox, should it be opened any other way.
//!
//! A page is served at `beepview://localhost/<token>/`, so what it links
//! by a relative URL (`chart.png`, `css/site.css`) is asked for under the
//! same token and read from beside the page on its machine, through the
//! same read path: only from the page's folder and the folders under it,
//! a bounded number of files and bytes per page. File previews use the
//! project root as their asset boundary and keep the entry path in the URL,
//! allowing parent-relative dependencies within the project.
//!
//! Each HTML response starts with [`PANE_SCRIPT`] to forward pane shortcuts
//! and external link clicks from the sandboxed frame to the app.

use crate::{
    agents::files::read_part,
    chat::files::free_name,
    error::{DesktopError, Result},
    fleet::{Fleet, Machines, files as direct},
    markdown,
};
use base64::{Engine, engine::general_purpose::STANDARD};
use beep_proto::{AGENT_FILE_CHUNK, ReadWorkFile};
use percent_encoding::percent_decode_str;
use serde::{Deserialize, Serialize};
use std::{
    collections::VecDeque,
    io::{ErrorKind, Write},
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicU32, AtomicU64, Ordering},
    },
};
use tauri::{
    AppHandle, State,
    http::{Request, Response, StatusCode, header},
};
use tokio::sync::Semaphore;
use uuid::Uuid;

/// The scheme HTML artifacts are served from.
pub const SCHEME: &str = "beepview";

/// The largest artifact read, a page or a file beside it.
const MAX_ARTIFACT: u64 = 5 * 1024 * 1024;

/// How many pages are kept to serve; the oldest goes first.
const KEPT_PAGES: usize = 8;

/// How many files beside it one page may load.
const MAX_PAGE_FILES: u32 = 1024;

/// How many bytes of files beside it one page may load in all.
const MAX_PAGE_BYTES: u64 = 32 * 1024 * 1024;

/// How many files beside pages are read at once.
const PARALLEL_READS: usize = 4;

/// The longest relative path a page may ask for, and its most folders.
const MAX_REL_PATH: usize = 1024;
const MAX_REL_DEPTH: usize = 16;

/// What an HTML artifact's own response allows: its scripts and styles,
/// inline, from the web or from beside it, and nothing of Beep's.
/// `sandbox` repeats the frame's.
const PAGE_CSP: &str = "default-src 'none'; \
    script-src 'unsafe-inline' 'unsafe-eval' https: beepview:; \
    style-src 'unsafe-inline' https: beepview:; img-src data: blob: https: beepview:; \
    font-src data: https: beepview:; media-src data: blob: https: beepview:; \
    connect-src https: beepview: blob: data:; frame-src 'none'; form-action 'none'; \
    base-uri 'none'; sandbox allow-scripts";

const PANE_SCRIPT: &[u8] = include_bytes!("preview-script.html");

const MAX_EXPORT_BYTES: usize = 16 * 1024 * 1024;

#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PreviewExport {
    Svg,
    Pdf,
}

impl PreviewExport {
    fn extension(self) -> &'static str {
        match self {
            Self::Svg => "svg",
            Self::Pdf => "pdf",
        }
    }
}

/// Saves bytes from an HTML preview to this device's usual download folder.
#[tauri::command]
pub async fn agent_preview_export(
    app: AppHandle,
    name: String,
    kind: PreviewExport,
    data: String,
) -> Result<PathBuf> {
    tauri::async_runtime::spawn_blocking(move || {
        save_preview_export(&crate::platform::saves_dir(&app)?, &name, kind, &data)
    })
    .await
    .map_err(anyhow::Error::from)?
}

fn save_preview_export(dir: &Path, name: &str, kind: PreviewExport, data: &str) -> Result<PathBuf> {
    let too_large = || DesktopError::BadRequest("The export is larger than 16 MB.".into());
    if data.len() > MAX_EXPORT_BYTES.div_ceil(3) * 4 {
        return Err(too_large());
    }

    let bytes = STANDARD
        .decode(data)
        .map_err(|_| DesktopError::BadRequest("Could not read the export.".into()))?;
    if bytes.len() > MAX_EXPORT_BYTES {
        return Err(too_large());
    }
    if bytes.is_empty() || (matches!(kind, PreviewExport::Pdf) && !bytes.starts_with(b"%PDF-")) {
        return Err(DesktopError::BadRequest(
            "The export has invalid file contents.".into(),
        ));
    }

    std::fs::create_dir_all(dir).map_err(anyhow::Error::from)?;
    let mut temp = tempfile::NamedTempFile::new_in(dir).map_err(anyhow::Error::from)?;
    temp.write_all(&bytes).map_err(anyhow::Error::from)?;
    let name = export_name(name, kind);
    let path = free_name(dir, &name);

    match temp.persist_noclobber(&path) {
        Ok(_) => Ok(path),
        // A concurrent save or a dangling symlink can occupy the chosen name.
        Err(error) if error.error.kind() == ErrorKind::AlreadyExists => {
            let path = dir.join(format!("{} {name}", Uuid::new_v4()));
            error
                .file
                .persist_noclobber(&path)
                .map_err(anyhow::Error::from)?;
            Ok(path)
        }
        Err(error) => Err(anyhow::Error::from(error).into()),
    }
}

fn export_name(name: &str, kind: PreviewExport) -> String {
    let base = name.rsplit(['/', '\\']).next().unwrap_or("");
    let stem = base.rsplit_once('.').map_or(base, |(stem, _)| stem);
    let stem: String = stem
        .chars()
        .take(48)
        .map(|ch| {
            if ch.is_control() || "<>:\"|?*".contains(ch) {
                '_'
            } else {
                ch
            }
        })
        .collect();
    let stem = stem.trim_matches([' ', '.']);
    let stem = if stem.is_empty() { "export" } else { stem };
    format!("{stem}.{}", kind.extension())
}

/// The HTML pages ready to serve, by the token in their URL.
pub struct Pages {
    kept: Mutex<VecDeque<Arc<Page>>>,
    /// Bounds the files beside pages read at once.
    reads: Arc<Semaphore>,
}

impl Default for Pages {
    fn default() -> Self {
        Self {
            kept: Mutex::default(),
            reads: Arc::new(Semaphore::new(PARALLEL_READS)),
        }
    }
}

struct Page {
    token: Uuid,
    entry: String,
    html: Arc<Vec<u8>>,
    place: Place,
    /// Files beside it asked for so far.
    files: AtomicU32,
    /// Bytes of those files read so far.
    bytes: AtomicU64,
    /// Where in its root each file it asked for is, once each, so the pane
    /// can tell the page changed when one of them did.
    loaded: Mutex<Vec<String>>,
}

/// Where a page was read from, so the files beside it are read there too.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Place {
    machine_id: String,
    root: String,
    /// The page's folder as its path gives it, with its last `/`, or ""
    /// for a page named without a folder.
    folder: String,
}

impl Place {
    fn of(machine_id: String, root: String, path: &str) -> Self {
        let folder = path
            .rfind('/')
            .map_or_else(String::new, |at| path[..=at].to_owned());
        Self {
            machine_id,
            root,
            folder,
        }
    }

    /// The file `rel` under the page's folder.
    fn file(&self, rel: &RelPath) -> Sibling {
        Sibling {
            machine_id: self.machine_id.clone(),
            root: self.root.clone(),
            path: format!("{}{}", self.folder, rel.0),
        }
    }
}

/// A file beside a page to read: on `machine_id`, `path` in `root`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sibling {
    machine_id: String,
    root: String,
    path: String,
}

/// A path under a page's folder as a request names it, decoded: no
/// segment is empty, `.` or `..`, or holds a separator, so it cannot leave
/// the folder.
#[derive(Debug, Clone, PartialEq, Eq)]
struct RelPath(String);

impl RelPath {
    fn parse(raw: &str) -> Option<Self> {
        let mut segments = Vec::new();

        for segment in raw.split('/') {
            let segment = percent_decode_str(segment).decode_utf8().ok()?;
            let bad = segment.is_empty()
                || segment == "."
                || segment == ".."
                || segment.contains(['/', '\\', '\0']);
            if bad {
                return None;
            }

            segments.push(segment);
        }

        let path = segments.join("/");
        (segments.len() <= MAX_REL_DEPTH && path.len() <= MAX_REL_PATH).then_some(Self(path))
    }
}

impl Pages {
    /// Keeps `html`, read from `place`, to serve, returning its URL; a
    /// preview's `entry` is its file in the URL.
    fn keep(&self, html: Vec<u8>, place: Place, entry: String) -> String {
        let token = Uuid::new_v4();
        let mut kept = self.kept.lock().unwrap_or_else(|e| e.into_inner());
        kept.push_back(Arc::new(Page {
            token,
            entry: entry.clone(),
            html: Arc::new(html),
            place,
            files: AtomicU32::new(0),
            bytes: AtomicU64::new(0),
            loaded: Mutex::default(),
        }));

        while kept.len() > KEPT_PAGES {
            kept.pop_front();
        }

        let mut url = url::Url::parse(&format!("{SCHEME}://localhost/{token}/"))
            .expect("the fixed preview URL is valid");
        if !entry.is_empty() {
            url.path_segments_mut()
                .expect("preview URLs have path segments")
                .pop_if_empty()
                .extend(entry.split('/'));
        }
        url.to_string()
    }

    fn get(&self, token: Uuid) -> Option<Arc<Page>> {
        let kept = self.kept.lock().unwrap_or_else(|e| e.into_inner());
        kept.iter().find(|page| page.token == token).cloned()
    }

    /// The files the page served at `src` asked for, by their path in its
    /// root; none for a page no longer kept.
    fn loaded(&self, src: &str) -> Vec<String> {
        let token = url::Url::parse(src)
            .ok()
            .and_then(|url| url.path_segments()?.next()?.parse::<Uuid>().ok());
        token
            .and_then(|token| self.get(token))
            .map(|page| {
                page.loaded
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .clone()
            })
            .unwrap_or_default()
    }
}

/// The files the HTML page shown at `src` loaded from beside it, by their
/// path in the root it was read from, for the pane to watch with it.
#[tauri::command]
pub fn agent_page_files(pages: State<'_, Pages>, src: String) -> Vec<String> {
    pages.loaded(&src)
}

/// How an artifact shows, by its extension.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Html,
    Markdown,
    Image(&'static str),
}

fn kind_of(path: &str) -> Option<Kind> {
    let name = path.rsplit('/').next()?;
    let (stem, ext) = name.rsplit_once('.')?;

    if stem.is_empty() {
        return None;
    }

    Some(match ext.to_ascii_lowercase().as_str() {
        "html" | "htm" => Kind::Html,
        "md" | "markdown" => Kind::Markdown,
        "svg" => Kind::Image("image/svg+xml"),
        "png" => Kind::Image("image/png"),
        "jpg" | "jpeg" => Kind::Image("image/jpeg"),
        "gif" => Kind::Image("image/gif"),
        "webp" => Kind::Image("image/webp"),
        _ => return None,
    })
}

/// An artifact ready to show.
#[derive(Debug, Serialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum ArtifactView {
    /// Shown in a sandboxed frame at `src`.
    Html { size: u64, src: String },
    /// Rendered and cleaned of anything that runs.
    Md { size: u64, html: String },
    /// Its bytes in base64, for an `<img>`; a picture or an SVG.
    Image {
        size: u64,
        mime: String,
        data: String,
    },
}

/// The artifact `path` in `root` on `machine_id`, read whole and made
/// ready to show.
#[tauri::command]
pub async fn agent_artifact(
    fleet: Machines<'_>,
    pages: State<'_, Pages>,
    machine_id: String,
    root: String,
    path: String,
    preview: Option<bool>,
) -> Result<ArtifactView> {
    let kind = kind_of(&path).ok_or_else(|| {
        DesktopError::BadRequest("Only pages, documents and pictures show here.".into())
    })?;
    let is_preview = preview.unwrap_or(false);
    let mut place = Place::of(machine_id.clone(), root.clone(), &path);
    let entry = if is_preview {
        let relative = path
            .strip_prefix(&format!("{}/", root.trim_end_matches('/')))
            .unwrap_or(&path);
        // Validate decoded filesystem segments before encoding them in the URL.
        if relative.starts_with('/')
            || relative.split('/').any(|part| {
                part.is_empty() || part == "." || part == ".." || part.contains(['\\', '\0'])
            })
        {
            return Err(DesktopError::BadRequest(
                "Preview must be inside the project.".into(),
            ));
        }
        place.folder.clear();
        relative.to_owned()
    } else {
        String::new()
    };
    let (data, size) = read_whole(&fleet, machine_id, root, path).await?;
    Ok(match kind {
        Kind::Html => ArtifactView::Html {
            size,
            src: pages.keep(data, place, entry),
        },
        Kind::Markdown => ArtifactView::Md {
            size,
            html: markdown::to_html(&String::from_utf8_lossy(&data)),
        },
        Kind::Image(mime) => ArtifactView::Image {
            size,
            mime: mime.into(),
            data: STANDARD.encode(&data),
        },
    })
}

/// Whether each of `paths` (absolute or `~/`) is still on `machine_id`, in
/// order. `None` when the machine cannot say now: it is offline, outside the
/// mesh or older, and then nothing shows as removed.
#[tauri::command]
pub async fn agent_artifacts_present(
    fleet: Machines<'_>,
    machine_id: String,
    paths: Vec<String>,
) -> Result<Option<Vec<bool>>> {
    let Some(machine) = fleet.member(&machine_id) else {
        return Ok(None);
    };

    match direct::present(&fleet, machine, paths).await {
        Ok(present) => Ok(present),
        Err(e) => {
            log::debug!("{machine_id} did not say which artifacts it has: {e}");
            Ok(None)
        }
    }
}

/// All of `path` in `root`, a part at a time, and its size; refused past
/// `MAX_ARTIFACT`.
async fn read_whole(
    fleet: &Fleet,
    machine_id: String,
    root: String,
    path: String,
) -> Result<(Vec<u8>, u64)> {
    let len = u32::try_from(AGENT_FILE_CHUNK).map_err(anyhow::Error::from)?;
    let mut data = Vec::new();

    loop {
        let offset = u64::try_from(data.len()).map_err(anyhow::Error::from)?;
        let part = read_part(
            fleet,
            ReadWorkFile {
                machine_id: machine_id.clone(),
                root: root.clone(),
                path: path.clone(),
                offset,
                len,
            },
        )
        .await?;

        if part.size > MAX_ARTIFACT {
            return Err(DesktopError::BadRequest(format!(
                "It is {} MB; artifacts up to {} MB show here.",
                part.size.div_ceil(1024 * 1024),
                MAX_ARTIFACT / (1024 * 1024)
            )));
        }

        if part.data.is_empty() {
            return Ok((data, offset));
        }

        data.extend_from_slice(&part.data);
        let have = u64::try_from(data.len()).map_err(anyhow::Error::from)?;

        if have >= part.size {
            return Ok((data, have));
        }

        if have > MAX_ARTIFACT {
            return Err(DesktopError::BadRequest(
                "The file grew while it was read.".into(),
            ));
        }
    }
}

/// What a `beepview:` request gets: an answer now, or a file beside a page
/// to read first.
pub enum Route {
    Done(Response<Vec<u8>>),
    Read(FileRead),
}

/// A file beside a kept page, counted against the page's limits, to read
/// and serve.
pub struct FileRead {
    page: Arc<Page>,
    file: Sibling,
    mime: &'static str,
    reads: Arc<Semaphore>,
}

/// Routes `beepview://localhost/<token>/<path>`: with no path, the page
/// `agent_artifact` kept, with its CSP; with one, the file at that path
/// under the page's folder, while the page has not loaded too many.
/// Anything else is not found.
pub fn route(pages: &Pages, request: &Request<Vec<u8>>) -> Route {
    let path = request.uri().path().trim_start_matches('/');
    let (token, rest) = path.split_once('/').unwrap_or((path, ""));
    let Some(page) = token
        .parse::<Uuid>()
        .ok()
        .and_then(|token| pages.get(token))
    else {
        return Route::Done(status(StatusCode::NOT_FOUND));
    };

    let rel = if rest.is_empty() {
        None
    } else {
        RelPath::parse(rest)
    };
    if rest.is_empty() || rel.as_ref().is_some_and(|path| path.0 == page.entry) {
        return Route::Done(respond(
            Response::builder()
                .status(StatusCode::OK)
                .header(header::CONTENT_TYPE, "text/html; charset=utf-8")
                .header(header::CONTENT_SECURITY_POLICY, PAGE_CSP)
                .header(header::REFERRER_POLICY, "no-referrer")
                .header(header::X_CONTENT_TYPE_OPTIONS, "nosniff")
                .body(with_pane_script(&page.html)),
        ));
    }

    let Some(rel) = rel else {
        return Route::Done(status(StatusCode::NOT_FOUND));
    };

    if page.files.fetch_add(1, Ordering::Relaxed) >= MAX_PAGE_FILES {
        return Route::Done(status(StatusCode::TOO_MANY_REQUESTS));
    }

    let file = page.place.file(&rel);
    let mut loaded = page.loaded.lock().unwrap_or_else(|e| e.into_inner());
    if !loaded.contains(&file.path) {
        loaded.push(file.path.clone());
    }
    drop(loaded);

    Route::Read(FileRead {
        file,
        mime: mime_of(&rel.0),
        page,
        reads: pages.reads.clone(),
    })
}

impl FileRead {
    /// Reads the file with `read` and answers with it; not found when it
    /// cannot be read, too large past the page's bytes.
    pub async fn serve<F, Fut>(self, read: F) -> Response<Vec<u8>>
    where
        F: FnOnce(Sibling) -> Fut,
        Fut: Future<Output = Result<Vec<u8>>>,
    {
        let Ok(_permit) = self.reads.acquire().await else {
            return status(StatusCode::SERVICE_UNAVAILABLE);
        };
        let path = self.file.path.clone();
        let data = match read(self.file).await {
            Ok(data) => data,
            Err(e) => {
                log::debug!("an artifact's file {path} is not read: {e}");
                return status(StatusCode::NOT_FOUND);
            }
        };
        let len = u64::try_from(data.len()).unwrap_or(u64::MAX);
        let before = self.page.bytes.fetch_add(len, Ordering::Relaxed);

        if before.saturating_add(len) > MAX_PAGE_BYTES {
            return status(StatusCode::PAYLOAD_TOO_LARGE);
        }

        let body = if self.mime.starts_with("text/html") {
            with_pane_script(&data)
        } else {
            data
        };

        respond(
            Response::builder()
                .status(StatusCode::OK)
                .header(header::CONTENT_TYPE, self.mime)
                .header(header::CONTENT_SECURITY_POLICY, PAGE_CSP)
                .header(header::X_CONTENT_TYPE_OPTIONS, "nosniff")
                // The sandboxed page's origin is opaque; its module
                // scripts and fonts load with CORS.
                .header(header::ACCESS_CONTROL_ALLOW_ORIGIN, "*")
                .body(body),
        )
    }
}

/// A file beside a page, read whole through the Files tab's read path.
pub async fn read_sibling(fleet: &Fleet, file: Sibling) -> Result<Vec<u8>> {
    let (data, _) = read_whole(fleet, file.machine_id, file.root, file.path).await?;
    Ok(data)
}

/// `html` with [`PANE_SCRIPT`] after its doctype, so the page stays in
/// standards mode, or at its start (after a byte order mark) without one.
fn with_pane_script(html: &[u8]) -> Vec<u8> {
    let start = if html.starts_with(b"\xEF\xBB\xBF") {
        3
    } else {
        0
    };
    let lead = html[start..]
        .iter()
        .position(|byte| !byte.is_ascii_whitespace())
        .map_or(html.len(), |at| start + at);
    let doctype = html[lead..]
        .get(..9)
        .is_some_and(|head| head.eq_ignore_ascii_case(b"<!doctype"));
    let at = if doctype {
        html[lead..]
            .iter()
            .position(|&byte| byte == b'>')
            .map_or(start, |end| lead + end + 1)
    } else {
        start
    };

    [&html[..at], PANE_SCRIPT, &html[at..]].concat()
}

/// The media type a file beside a page is served as, by its extension.
fn mime_of(path: &str) -> &'static str {
    let ext = path
        .rsplit_once('.')
        .map(|(_, ext)| ext.to_ascii_lowercase())
        .unwrap_or_default();
    match ext.as_str() {
        "html" | "htm" => "text/html; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "js" | "mjs" => "text/javascript; charset=utf-8",
        "json" | "map" => "application/json",
        "txt" | "md" => "text/plain; charset=utf-8",
        "csv" => "text/csv; charset=utf-8",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "avif" => "image/avif",
        "ico" => "image/x-icon",
        "woff" => "font/woff",
        "woff2" => "font/woff2",
        "ttf" => "font/ttf",
        "otf" => "font/otf",
        "mp4" => "video/mp4",
        "webm" => "video/webm",
        "mp3" => "audio/mpeg",
        "wav" => "audio/wav",
        "ogg" => "audio/ogg",
        "wasm" => "application/wasm",
        "pdf" => "application/pdf",
        _ => "application/octet-stream",
    }
}

/// The answer while Beep has not started.
pub fn unavailable() -> Response<Vec<u8>> {
    status(StatusCode::SERVICE_UNAVAILABLE)
}

fn status(code: StatusCode) -> Response<Vec<u8>> {
    respond(Response::builder().status(code).body(Vec::new()))
}

fn respond(built: tauri::http::Result<Response<Vec<u8>>>) -> Response<Vec<u8>> {
    built.unwrap_or_else(|e| {
        log::warn!("could not serve an artifact: {e}");
        Response::new(Vec::new())
    })
}

#[cfg(test)]
#[path = "../tests/agents_artifacts.rs"]
mod tests;
