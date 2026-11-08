//! Keep app windows on the bundled UI and deny web popups.

use tauri::{Manager, Runtime, WebviewWindowBuilder, webview::NewWindowResponse};
use url::Url;

pub(crate) fn guard<'a, R: Runtime, M: Manager<R>>(
    builder: WebviewWindowBuilder<'a, R, M>,
) -> WebviewWindowBuilder<'a, R, M> {
    builder
        .on_navigation(allowed)
        .on_new_window(|_, _| NewWindowResponse::Deny)
}

fn allowed(url: &Url) -> bool {
    if !url.username().is_empty() || url.password().is_some() || url.port().is_some() {
        return false;
    }
    // WebKit also sends iframe navigations here. Artifact pages have their
    // own origin and are sandboxed without top navigation or popup access.
    // Hidden previews navigate to about:blank to stop their scripts.
    if url.as_str() == "about:blank"
        || (url.scheme() == crate::agents::artifacts::SCHEME && url.host_str() == Some("localhost"))
    {
        return true;
    }
    let app_origin = if cfg!(target_os = "windows") {
        url.scheme() == "http" && url.host_str() == Some("tauri.localhost")
    } else {
        url.scheme() == "tauri" && url.host_str() == Some("localhost")
    };
    app_origin
        && matches!(
            url.path(),
            "" | "/" | "/index.html" | "/center.html" | "/orb.html"
        )
}

#[cfg(test)]
mod tests {
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
            "/orb.html",
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
}
