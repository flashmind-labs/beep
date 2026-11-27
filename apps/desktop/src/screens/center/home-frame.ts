// The HTML of the last Home, kept for the next launch: the app puts it on the page before the
// scripts load, and the first real view removes it. Saved when the window loses focus, not on every draw.
import { onPhone } from "../../ui/platform.ts";
import { dialogOpen } from "../../ui/parts.tsx";
import { invoke } from "../../ui/tauri.ts";
import { state } from "../../stores/center/state.ts";
import { currentLocation } from "../../parts/center/nav.ts";
import { frameOf } from "../../stores/center/home-frame-state.ts";

/// Keeps what Home shows now, when it is Home on a desktop window with its answer drawn.
function keepHomeFrame() {
  if (onPhone() || document.body.classList.contains("mobile") || document.body.classList.contains("pw")) return;
  if (currentLocation()?.view !== "home" || !state.home || dialogOpen()) return;

  const win = document.querySelector(".cwin:not(.kept)");
  if (!win) return;

  invoke("keep_home_frame", { frame: frameOf(win, document.documentElement.dataset.skin ?? null) })
    .catch(e => console.warn("keeping the Home frame:", e));
}

/// Keeps Home's frame when the window loses focus, as it does before it hides or minimizes.
export function followHomeFrame() {
  window.addEventListener("blur", keepHomeFrame);
}

/// Removes the frame the app put on the page, once the real first view is drawn.
export function dropKeptFrame() {
  for (const el of document.querySelectorAll(".cwin.kept")) el.remove();
}
