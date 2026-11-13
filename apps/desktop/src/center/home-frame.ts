// The HTML of the last Home, kept for the next launch: the app puts it on the page before the
// scripts load, and the first real view removes it. Saved when the window is left, not on every draw.
import { onPhone } from "../ui/platform.ts";
import { dialogOpen } from "../ui/parts.ts";
import { invoke } from "../ui/tauri.ts";
import { state } from "./state.ts";
import { currentLocation } from "./views.ts";
import { frameOf } from "./home-frame-state.ts";

/// Keeps what Home shows now, when it is Home on a desktop window with its answer drawn.
export function keepHomeFrame() {
  if (onPhone() || document.body.classList.contains("mobile") || document.body.classList.contains("pw")) return;
  if (currentLocation()?.view !== "home" || !state.home || dialogOpen()) return;

  const win = document.querySelector(".cwin");
  if (!win) return;

  invoke("keep_home_frame", { frame: frameOf(win, document.documentElement.dataset.skin ?? null) })
    .catch(e => console.warn("keeping the Home frame:", e));
}

/// Keeps Home's frame when the window loses focus or is hidden.
export function followHomeFrame() {
  window.addEventListener("blur", keepHomeFrame);
  document.addEventListener("visibilitychange", () => { if (document.hidden) keepHomeFrame(); });
}

/// Removes the frame the app put on the page, once the real first view is drawn.
export function dropKeptFrame() {
  for (const el of document.querySelectorAll(".cwin.kept")) el.remove();
}
