//! Keep app windows on the bundled UI and deny web popups.

use tauri::{Manager, Runtime, WebviewWindowBuilder, webview::NewWindowResponse};
use url::Url;

/// The scheme HTML artifacts are served from.
pub const SCHEME: &str = "beepview";

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
        || (url.scheme() == SCHEME && url.host_str() == Some("localhost"))
    {
        return true;
    }

    // WebView2 and Android's WebView serve the app over http, WebKit over its own scheme.
    let app_origin = if cfg!(any(target_os = "windows", target_os = "android")) {
        url.scheme() == "http" && url.host_str() == Some("tauri.localhost")
    } else {
        url.scheme() == "tauri" && url.host_str() == Some("localhost")
    };
    // WebKit asks about the sandboxed diagram iframe as well as app windows.
    app_origin
        && matches!(
            url.path(),
            "" | "/" | "/index.html" | "/center.html" | "/diagrams/index.html"
        )
}

#[cfg(test)]
#[path = "tests/navigation.rs"]
mod tests;
