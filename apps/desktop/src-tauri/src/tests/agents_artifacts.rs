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

#[tokio::test]
async fn file_previews_load_scripts_fonts_and_parent_assets_inside_the_project() {
    let pages = Pages::default();
    let mut at = place();
    at.folder.clear();
    let src = pages.keep(
        b"<script src='../mermaid.js'></script>".to_vec(),
        at,
        "mockups/desktop/index.html".into(),
    );
    let page = done(&pages, &src);
    assert_eq!(page.headers()[header::CONTENT_SECURITY_POLICY], PAGE_CSP);
    assert!(PAGE_CSP.contains("script-src"));
    assert!(PAGE_CSP.contains("font-src data: https: beepview:"));
    assert!(PAGE_CSP.ends_with("sandbox allow-scripts"));
    let url = url::Url::parse(&src).expect("preview URL");
    let asset = url.join("../mermaid.js").expect("relative URL");
    let Route::Read(file) = route(&pages, &request(asset.as_str())) else {
        panic!("parent asset must load");
    };
    let response = file
        .serve(|file| async move {
            assert_eq!(file.root, "/srv/project");
            assert_eq!(file.path, "mockups/mermaid.js");
            Ok(b"window.mermaid = {};".to_vec())
        })
        .await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response.headers()[header::CONTENT_TYPE],
        "text/javascript; charset=utf-8"
    );
    assert_eq!(
        done(
            &pages,
            url.join("../../../secret").expect("relative URL").as_str()
        )
        .status(),
        StatusCode::NOT_FOUND
    );
}

#[test]
fn preview_entry_paths_encode_spaces_and_percent_signs() {
    let pages = Pages::default();
    let src = pages.keep(b"page".to_vec(), place(), "reports/100% done.html".into());
    assert!(src.ends_with("reports/100%25%20done.html"));
    assert_eq!(done(&pages, &src).body(), b"page");
}

#[test]
fn a_kept_page_is_served_with_its_sandbox_and_others_are_not() {
    let pages = Pages::default();
    let src = pages.keep(b"<p>hi</p>".to_vec(), place(), String::new());
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
    let src = pages.keep(b"<img src=\"chart.png\">".to_vec(), place(), String::new());
    let image = fetch(&pages, &format!("{src}chart.png")).await;
    assert_eq!(image.status(), StatusCode::OK);
    assert_eq!(image.body(), b"\x89PNG");
    assert_eq!(image.headers()[header::CONTENT_TYPE], "image/png");
    assert_eq!(
        fetch(&pages, &format!("{src}gone.png")).await.status(),
        StatusCode::NOT_FOUND
    );
}

#[tokio::test]
async fn a_page_lists_the_files_it_loaded_once_each() {
    let pages = Pages::default();
    let src = pages.keep(b"<img src=\"chart.png\">".to_vec(), place(), String::new());
    assert!(pages.loaded(&src).is_empty());
    fetch(&pages, &format!("{src}chart.png")).await;
    fetch(&pages, &format!("{src}chart.png")).await;
    fetch(&pages, &format!("{src}css/site.css")).await;
    assert_eq!(
        pages.loaded(&src),
        [
            "/srv/project/reports/chart.png",
            "/srv/project/reports/css/site.css"
        ]
    );
    assert!(pages.loaded("beepview://localhost/not-a-token/").is_empty());
}

#[test]
fn a_path_out_of_the_folder_is_refused() {
    let pages = Pages::default();
    let src = pages.keep(b"<p>hi</p>".to_vec(), place(), String::new());
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
    let src = pages.keep(b"<p>hi</p>".to_vec(), place(), String::new());

    const { assert!(MAX_PAGE_FILES > 100, "large mocks need more than 64 assets") };
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
    let first = pages.keep(b"1".to_vec(), place(), String::new());

    for _ in 0..KEPT_PAGES {
        pages.keep(b"more".to_vec(), place(), String::new());
    }

    let token = |src: &str| {
        src.trim_end_matches('/')
            .rsplit('/')
            .next()
            .and_then(|t| t.parse::<Uuid>().ok())
            .expect("a token")
    };
    assert!(pages.get(token(&first)).is_none());
    let last = pages.keep(b"last".to_vec(), place(), String::new());
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
