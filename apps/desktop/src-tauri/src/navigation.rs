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
#[path = "tests/navigation.rs"]
mod tests;
