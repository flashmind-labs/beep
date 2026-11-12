import "./keys.ts";
import { linkedPath } from "../agentpane/media-state.ts";
import { copy } from "../ui/parts.ts";
import { bar, PANE } from "./core.ts";
import { openViewer } from "../chat/media.ts";
import { forgetHeight } from "./size.ts";
import { show } from "./views.ts";
import { copyCode } from "../agents/turnrows.ts";
import { COPY_ICON, ICON } from "../ui/icons.tsx";
import { invoke, listen } from "../ui/tauri.ts";

/// A button that says what it did for a moment, then goes back.
interface UndoButton extends HTMLElement {
  undo?: ReturnType<typeof setTimeout>;
}

// Only the Option double tap shows the bar without naming a view.
if (!PANE) listen<string | null>("bar-shown", e => { forgetHeight(); show(e.payload); });

/// Shows `done` in button `b` for a moment, then `back` again.
function flash(b: UndoButton, done: string, back: string) {
  b.innerHTML = done;
  clearTimeout(b.undo);
  b.undo = setTimeout(() => { b.innerHTML = back; }, 1200);
}

// Links in replies open in the browser. A product card's corner button
// copies its link instead.
bar.addEventListener("click", e => {
  const t = e.target as HTMLElement;
  // The sanitized markup of replies and cards brings its own copy buttons: a
  // product card's copies its link, a code block's or a quote's its text.
  const link = t.closest<UndoButton>(".dprod-copy[data-copy]");

  if (link) {
    e.preventDefault();
    copy(link.dataset.copy ?? "");
    return flash(link, ICON.check, COPY_ICON);
  }

  const code = t.closest<HTMLElement>(".dcoderen.agent [data-copy]");
  if (code) return copyCode(code);

  const img = t.closest<HTMLImageElement>(".dcoderen.agent img");

  if (img && !img.closest("a, .dprods, .drecs")) {
    return openViewer([{ url: img.currentSrc || img.src, caption: img.alt }], 0);
  }

  const a = t.closest<HTMLAnchorElement>("a[href]");
  if (!a) return;
  e.preventDefault();
  // A link to a file, not a web address: the session's pane opens it.
  if (linkedPath(a.getAttribute("href")!)) return;
  invoke("open_url", { url: a.href }).catch(() => {});
});

// Product pictures show over their placeholder once loaded; a broken one
// leaves the placeholder. The product cards React draws handle their own.
const isProductPic = (t: EventTarget | null): t is Element => t instanceof Element && t.matches(".dprod-img img, .drec img") && !t.closest(".files > .dprods");
bar.addEventListener("load", e => { if (isProductPic(e.target)) e.target.classList.add("on"); }, true);
bar.addEventListener("error", e => { if (isProductPic(e.target)) e.target.remove(); }, true);
