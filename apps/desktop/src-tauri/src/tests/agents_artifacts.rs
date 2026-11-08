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
async fn file_previews_restrict_the_page_and_its_siblings() {
    let pages = Pages::default();
    let src = pages.keep_with_policy(b"<img src=chart.png>".to_vec(), place(), true);
    let page = done(&pages, &src);
    assert_eq!(page.headers()[header::CONTENT_SECURITY_POLICY], PREVIEW_CSP);
    assert!(!PREVIEW_CSP.contains("https:"));
    assert!(!PREVIEW_CSP.contains("allow-scripts"));
    assert!(!PREVIEW_CSP.contains("script-src"));
    let image = fetch(&pages, &format!("{src}chart.png")).await;
    assert_eq!(image.status(), StatusCode::OK);
    assert_eq!(image.body(), b"\x89PNG");
    assert_eq!(
        image.headers()[header::CONTENT_SECURITY_POLICY],
        PREVIEW_CSP
    );
    assert_eq!(
        done(&pages, &format!("{src}%2e%2e/secret")).status(),
        StatusCode::NOT_FOUND
    );
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
