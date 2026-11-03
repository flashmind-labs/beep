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

use crate::{
    beep::Beep,
    error::{DesktopError, Result},
    markdown,
    work_files::read_part,
};
use base64::{Engine, engine::general_purpose::STANDARD};
use beep_proto::{AGENT_FILE_CHUNK, ReadWorkFile};
use serde::Serialize;
use std::{
    collections::VecDeque,
    sync::{Arc, Mutex},
};
use tauri::{
    State,
    http::{Request, Response, StatusCode, header},
};
use uuid::Uuid;

/// The scheme HTML artifacts are served from.
pub const SCHEME: &str = "beepview";

/// The largest artifact read.
const MAX_ARTIFACT: u64 = 5 * 1024 * 1024;

/// How many pages are kept to serve; the oldest goes first.
const KEPT_PAGES: usize = 8;

/// What an HTML artifact's own response allows: its scripts and styles,
/// inline or from the web, and nothing of Beep's. `sandbox` repeats the
/// frame's.
const PAGE_CSP: &str = "default-src 'none'; script-src 'unsafe-inline' 'unsafe-eval' https:; \
    style-src 'unsafe-inline' https:; img-src data: blob: https:; font-src data: https:; \
    media-src data: blob: https:; connect-src https:; frame-src 'none'; form-action 'none'; \
    base-uri 'none'; sandbox allow-scripts";

/// The HTML pages ready to serve, by the token in their URL.
#[derive(Default)]
pub struct Pages {
    kept: Mutex<VecDeque<Page>>,
}

struct Page {
    token: Uuid,
    html: Arc<Vec<u8>>,
}

impl Pages {
    /// Keeps `html` to serve, returning its URL.
    fn keep(&self, html: Vec<u8>) -> String {
        let token = Uuid::new_v4();
        let mut kept = self.kept.lock().unwrap_or_else(|e| e.into_inner());
        kept.push_back(Page {
            token,
            html: Arc::new(html),
        });
        while kept.len() > KEPT_PAGES {
            kept.pop_front();
        }
        format!("{SCHEME}://localhost/{token}")
    }

    fn get(&self, token: Uuid) -> Option<Arc<Vec<u8>>> {
        let kept = self.kept.lock().unwrap_or_else(|e| e.into_inner());
        kept.iter()
            .find(|page| page.token == token)
            .map(|page| page.html.clone())
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
    pages: State<'_, Pages>,
    machine_id: String,
    root: String,
    path: String,
) -> Result<ArtifactView> {
    let kind = kind_of(&path).ok_or_else(|| {
        DesktopError::BadRequest("Only pages, documents and pictures show here.".into())
    })?;
    let (data, size) = read_whole(&beep, machine_id, root, path).await?;
    Ok(match kind {
        Kind::Html => ArtifactView::Html {
            size,
            src: pages.keep(data),
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

/// All of `path` in `root`, a part at a time, and its size; refused past
/// `MAX_ARTIFACT`.
async fn read_whole(
    beep: &Beep,
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

/// Serves `beepview://localhost/<token>`: a page `agent_artifact` kept,
/// with its CSP; anything else is not found.
pub fn serve(pages: &Pages, request: &Request<Vec<u8>>) -> Response<Vec<u8>> {
    let page = request
        .uri()
        .path()
        .trim_start_matches('/')
        .parse::<Uuid>()
        .ok()
        .and_then(|token| pages.get(token));
    let built = match page {
        Some(html) => Response::builder()
            .status(StatusCode::OK)
            .header(header::CONTENT_TYPE, "text/html; charset=utf-8")
            .header(header::CONTENT_SECURITY_POLICY, PAGE_CSP)
            .header(header::REFERRER_POLICY, "no-referrer")
            .header(header::X_CONTENT_TYPE_OPTIONS, "nosniff")
            .body(html.as_ref().clone()),
        None => Response::builder()
            .status(StatusCode::NOT_FOUND)
            .body(Vec::new()),
    };
    built.unwrap_or_else(|e| {
        log::warn!("could not serve an artifact: {e}");
        Response::new(Vec::new())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_kept_page_is_served_with_its_sandbox_and_others_are_not() {
        let pages = Pages::default();
        let src = pages.keep(b"<p>hi</p>".to_vec());
        let path = src
            .strip_prefix("beepview://localhost")
            .expect("served on beepview");
        let ask = |path: &str| {
            let request = Request::builder()
                .uri(format!("beepview://localhost{path}"))
                .body(Vec::new())
                .expect("a valid request");
            serve(&pages, &request)
        };
        let page = ask(path);
        assert_eq!(page.status(), StatusCode::OK);
        assert_eq!(page.body(), b"<p>hi</p>");
        let csp = page.headers()[header::CONTENT_SECURITY_POLICY]
            .to_str()
            .expect("ASCII");
        assert!(csp.ends_with("sandbox allow-scripts"), "{csp}");
        assert_eq!(ask("/not-a-token").status(), StatusCode::NOT_FOUND);
        assert_eq!(
            ask(&format!("/{}", Uuid::new_v4())).status(),
            StatusCode::NOT_FOUND
        );
    }

    #[test]
    fn only_the_oldest_pages_go() {
        let pages = Pages::default();
        let first = pages.keep(b"1".to_vec());
        for _ in 0..KEPT_PAGES {
            pages.keep(b"more".to_vec());
        }
        let token = |src: &str| {
            src.rsplit('/')
                .next()
                .and_then(|t| t.parse::<Uuid>().ok())
                .expect("a token")
        };
        assert!(pages.get(token(&first)).is_none());
        let last = pages.keep(b"last".to_vec());
        assert_eq!(
            pages.get(token(&last)).as_deref().map(Vec::as_slice),
            Some(&b"last"[..])
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
