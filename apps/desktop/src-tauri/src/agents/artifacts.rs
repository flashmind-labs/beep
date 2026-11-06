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
//! a bounded number of files and bytes per page.

use crate::{
    agents::files::read_part,
    beep::Beep,
    chat::markdown,
    error::{DesktopError, Result},
    fleet::{Fleet, Machines, files as direct},
};
use base64::{Engine, engine::general_purpose::STANDARD};
use beep_proto::{AGENT_FILE_CHUNK, ReadWorkFile};
use percent_encoding::percent_decode_str;
use serde::Serialize;
use std::{
    collections::VecDeque,
    sync::{
        Arc, Mutex,
        atomic::{AtomicU32, AtomicU64, Ordering},
    },
};
use tauri::{
    State,
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
const MAX_PAGE_FILES: u32 = 64;

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
    connect-src https: beepview:; frame-src 'none'; form-action 'none'; \
    base-uri 'none'; sandbox allow-scripts";

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
    html: Arc<Vec<u8>>,
    place: Place,
    /// Files beside it asked for so far.
    files: AtomicU32,
    /// Bytes of those files read so far.
    bytes: AtomicU64,
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
    /// Keeps `html`, read from `place`, to serve, returning its URL.
    fn keep(&self, html: Vec<u8>, place: Place) -> String {
        let token = Uuid::new_v4();
        let mut kept = self.kept.lock().unwrap_or_else(|e| e.into_inner());
        kept.push_back(Arc::new(Page {
            token,
            html: Arc::new(html),
            place,
            files: AtomicU32::new(0),
            bytes: AtomicU64::new(0),
        }));
        while kept.len() > KEPT_PAGES {
            kept.pop_front();
        }
        format!("{SCHEME}://localhost/{token}/")
    }

    fn get(&self, token: Uuid) -> Option<Arc<Page>> {
        let kept = self.kept.lock().unwrap_or_else(|e| e.into_inner());
        kept.iter().find(|page| page.token == token).cloned()
    }
}

/// How an artifact shows, by its extension.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Html,
    Markdown,
    Svg,
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
        "svg" => Kind::Svg,
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
    /// Its bytes in base64, for an `<img>`.
    Svg {
        size: u64,
        mime: String,
        data: String,
    },
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
    beep: State<'_, Arc<Beep>>,
    fleet: Machines<'_>,
    pages: State<'_, Pages>,
    machine_id: String,
    root: String,
    path: String,
) -> Result<ArtifactView> {
    let kind = kind_of(&path).ok_or_else(|| {
        DesktopError::BadRequest("Only pages, documents and pictures show here.".into())
    })?;
    let place = Place::of(machine_id.clone(), root.clone(), &path);
    let (data, size) = read_whole(&beep, &fleet, machine_id, root, path).await?;
    Ok(match kind {
        Kind::Html => ArtifactView::Html {
            size,
            src: pages.keep(data, place),
        },
        Kind::Markdown => ArtifactView::Md {
            size,
            html: markdown::to_html(&String::from_utf8_lossy(&data)),
        },
        Kind::Svg => ArtifactView::Svg {
            size,
            mime: "image/svg+xml".into(),
            data: STANDARD.encode(&data),
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
    beep: &Beep,
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
            beep,
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
    if rest.is_empty() {
        return Route::Done(respond(
            Response::builder()
                .status(StatusCode::OK)
                .header(header::CONTENT_TYPE, "text/html; charset=utf-8")
                .header(header::CONTENT_SECURITY_POLICY, PAGE_CSP)
                .header(header::REFERRER_POLICY, "no-referrer")
                .header(header::X_CONTENT_TYPE_OPTIONS, "nosniff")
                .body(page.html.as_ref().clone()),
        ));
    }
    let Some(rel) = RelPath::parse(rest) else {
        return Route::Done(status(StatusCode::NOT_FOUND));
    };
    if page.files.fetch_add(1, Ordering::Relaxed) >= MAX_PAGE_FILES {
        return Route::Done(status(StatusCode::TOO_MANY_REQUESTS));
    }
    Route::Read(FileRead {
        file: page.place.file(&rel),
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
        respond(
            Response::builder()
                .status(StatusCode::OK)
                .header(header::CONTENT_TYPE, self.mime)
                .header(header::X_CONTENT_TYPE_OPTIONS, "nosniff")
                // The sandboxed page's origin is opaque; its module
                // scripts and fonts load with CORS.
                .header(header::ACCESS_CONTROL_ALLOW_ORIGIN, "*")
                .body(data),
        )
    }
}

/// A file beside a page, read whole through the Files tab's read path.
pub async fn read_sibling(beep: &Beep, fleet: &Fleet, file: Sibling) -> Result<Vec<u8>> {
    let (data, _) = read_whole(beep, fleet, file.machine_id, file.root, file.path).await?;
    Ok(data)
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
mod tests {
    use super::*;

    const PAGE: &str = "/srv/project/reports/usage.html";

    fn place() -> Place {
        Place::of("m1".into(), "/srv/project".into(), PAGE)
    }

    fn request(url: &str) -> Request<Vec<u8>> {
        Request::builder()
            .uri(url)
            .body(Vec::new())
            .expect("a valid request")
    }

    /// The answer `route` gives now; panics on a file to read.
    fn done(pages: &Pages, url: &str) -> Response<Vec<u8>> {
        match route(pages, &request(url)) {
            Route::Done(response) => response,
            Route::Read(file) => panic!("{url} reads {:?}", file.file),
        }
    }

    /// Serves `url` with a machine that has only `/srv/project/reports/chart.png`.
    async fn fetch(pages: &Pages, url: &str) -> Response<Vec<u8>> {
        match route(pages, &request(url)) {
            Route::Done(response) => response,
            Route::Read(file) => {
                file.serve(|at| async move {
                    assert_eq!(at.machine_id, "m1");
                    assert_eq!(at.root, "/srv/project");
                    if at.path == "/srv/project/reports/chart.png" {
                        Ok(b"\x89PNG".to_vec())
                    } else {
                        Err(DesktopError::BadRequest("not there".into()))
                    }
                })
                .await
            }
        }
    }

    #[test]
    fn a_kept_page_is_served_with_its_sandbox_and_others_are_not() {
        let pages = Pages::default();
        let src = pages.keep(b"<p>hi</p>".to_vec(), place());
        assert!(src.ends_with('/'), "relative links resolve under {src}");
        let page = done(&pages, &src);
        assert_eq!(page.status(), StatusCode::OK);
        assert_eq!(page.body(), b"<p>hi</p>");
        let csp = page.headers()[header::CONTENT_SECURITY_POLICY]
            .to_str()
            .expect("ASCII");
        assert!(csp.ends_with("sandbox allow-scripts"), "{csp}");
        assert!(
            csp.contains("img-src data: blob: https: beepview:"),
            "{csp}"
        );
        // The URL without its slash still finds the page.
        assert_eq!(
            done(&pages, src.trim_end_matches('/')).status(),
            StatusCode::OK
        );
        let missing = |path: &str| done(&pages, &format!("beepview://localhost{path}")).status();
        assert_eq!(missing("/not-a-token"), StatusCode::NOT_FOUND);
        assert_eq!(
            missing(&format!("/{}", Uuid::new_v4())),
            StatusCode::NOT_FOUND
        );
        assert_eq!(
            missing(&format!("/{}/chart.png", Uuid::new_v4())),
            StatusCode::NOT_FOUND
        );
    }

    #[tokio::test]
    async fn a_relative_image_is_read_from_beside_the_page() {
        let pages = Pages::default();
        let src = pages.keep(b"<img src=\"chart.png\">".to_vec(), place());
        let image = fetch(&pages, &format!("{src}chart.png")).await;
        assert_eq!(image.status(), StatusCode::OK);
        assert_eq!(image.body(), b"\x89PNG");
        assert_eq!(image.headers()[header::CONTENT_TYPE], "image/png");
        assert_eq!(
            fetch(&pages, &format!("{src}gone.png")).await.status(),
            StatusCode::NOT_FOUND
        );
    }

    #[test]
    fn a_path_out_of_the_folder_is_refused() {
        let pages = Pages::default();
        let src = pages.keep(b"<p>hi</p>".to_vec(), place());
        for escape in [
            "%2e%2e/secret.txt",
            "..%2fsecret.txt",
            "css/%2E%2E/%2E%2E/secret.txt",
            "%2e%2e%5csecret.txt",
            "./chart.png",
            "css//site.css",
            "css/",
        ] {
            assert_eq!(
                done(&pages, &format!("{src}{escape}")).status(),
                StatusCode::NOT_FOUND,
                "{escape}"
            );
        }
    }

    #[test]
    fn files_resolve_under_the_page_folder() {
        let rel = |raw: &str| RelPath::parse(raw).expect("a path under the folder");
        assert_eq!(
            place().file(&rel("css/site%20main.css")).path,
            "/srv/project/reports/css/site main.css"
        );
        let bare = Place::of("m1".into(), "/srv/project".into(), "usage.html");
        assert_eq!(bare.file(&rel("chart.png")).path, "chart.png");
        assert_eq!(RelPath::parse(".."), None);
        assert_eq!(RelPath::parse(&"a/".repeat(MAX_REL_DEPTH + 1)), None);
    }

    #[tokio::test]
    async fn a_page_loads_a_bounded_number_of_files() {
        let pages = Pages::default();
        let src = pages.keep(b"<p>hi</p>".to_vec(), place());
        for _ in 0..MAX_PAGE_FILES {
            assert_eq!(
                fetch(&pages, &format!("{src}chart.png")).await.status(),
                StatusCode::OK
            );
        }
        assert_eq!(
            done(&pages, &format!("{src}chart.png")).status(),
            StatusCode::TOO_MANY_REQUESTS
        );
    }

    #[test]
    fn only_the_oldest_pages_go() {
        let pages = Pages::default();
        let first = pages.keep(b"1".to_vec(), place());
        for _ in 0..KEPT_PAGES {
            pages.keep(b"more".to_vec(), place());
        }
        let token = |src: &str| {
            src.trim_end_matches('/')
                .rsplit('/')
                .next()
                .and_then(|t| t.parse::<Uuid>().ok())
                .expect("a token")
        };
        assert!(pages.get(token(&first)).is_none());
        let last = pages.keep(b"last".to_vec(), place());
        assert_eq!(
            pages
                .get(token(&last))
                .map(|page| page.html.as_ref().clone()),
            Some(b"last".to_vec())
        );
    }

    #[test]
    fn kinds_go_by_extension() {
        assert_eq!(kind_of("/srv/project/a.HTML"), Some(Kind::Html));
        assert_eq!(kind_of("notes.md"), Some(Kind::Markdown));
        assert_eq!(kind_of("flow.svg"), Some(Kind::Svg));
        assert_eq!(kind_of("shot.jpeg"), Some(Kind::Image("image/jpeg")));
        assert_eq!(kind_of("main.rs"), None);
        assert_eq!(kind_of(".html"), None);
    }
}
