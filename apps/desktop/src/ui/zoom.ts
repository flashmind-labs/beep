// ⌘= ⌘- ⌘0 in every window. The level is the app's, kept by Rust, which
// zooms every window; each then tells its terminals to fit again. Every
// window's entry imports this.

import { invoke, listen } from "./tauri.ts";
import { onPhone } from "./platform.ts";
import { nextZoom, zoomKey, ZOOMED } from "./zoom-state.ts";

// Capture, so it comes before a terminal or any page's own keys.
if (!onPhone()) document.addEventListener("keydown", e => {
  const key = zoomKey(e);
  if (!key) return;
  e.preventDefault();
  e.stopPropagation();
  invoke<number>("zoom_level")
    .then(at => invoke("set_zoom_level", { percent: nextZoom(at, key) }))
    .catch(() => {});
}, true);

listen("zoom", () => requestAnimationFrame(() => window.dispatchEvent(new Event(ZOOMED))));
