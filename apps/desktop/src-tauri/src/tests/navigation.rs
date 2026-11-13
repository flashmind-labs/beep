use super::*;

#[test]
fn bundled_pages_can_load_reload_and_use_fragments() {
    let origin = if cfg!(target_os = "windows") {
        "http://tauri.localhost"
    } else {
        "tauri://localhost"
    };
    for path in [
        "/",
        "/index.html",
        "/center.html?project=example#chat",
        "/diagrams/index.html",
    ] {
        assert!(allowed(&Url::parse(&format!("{origin}{path}")).unwrap()));
    }
}

#[test]
fn links_cannot_replace_the_ui() {
    for link in [
        "https://example.com/",
        "http://localhost/center.html",
        "https://tauri.localhost/center.html",
        "tauri://example.com/center.html",
        "tauri://localhost.example.com/center.html",
        "tauri://localhost:8080/center.html",
        "tauri://user@localhost/center.html",
        "tauri://localhost/other.html",
        "tauri://localhost/diagrams/other.html",
        "file:///index.html",
        "data:text/html,hello",
        "javascript:alert(1)",
        "blob:tauri://localhost/example",
    ] {
        assert!(!allowed(&Url::parse(link).unwrap()), "{link}");
    }
}

#[test]
fn sandboxed_artifact_frames_can_load() {
    assert!(allowed(&Url::parse("about:blank").unwrap()));
    assert!(allowed(
        &Url::parse("beepview://localhost/example/").unwrap()
    ));
    assert!(!allowed(
        &Url::parse("beepview://example.com/example/").unwrap()
    ));
}
