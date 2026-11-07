// Suppress the webview menu without stopping custom context menu handlers.
document.addEventListener("contextmenu", e => e.preventDefault(), true);
