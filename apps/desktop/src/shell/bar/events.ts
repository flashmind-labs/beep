import "./keys.ts";
import { linkedPath } from "../../stores/agentpane/media-state.ts";
import { linkedSession } from "../../stores/agents/nesting-state.ts";
import { bar, PANE } from "../../stores/bar/core.ts";
import { openViewer } from "../../parts/session/media.tsx";
import { forgetHeight } from "../../stores/bar/size.ts";
import { show } from "./views.ts";
import { copyCode } from "../../parts/session/turnrows.ts";
import { invoke, listen } from "../../ui/tauri.ts";

// Only the Option double tap shows the bar without naming a view.
if (!PANE) listen<string | null>("bar-shown", e => { forgetHeight(); show(e.payload); });

// Links in replies open in the browser.
bar.addEventListener("click", e => {
  const t = e.target as HTMLElement;
  // The sanitized markup of replies brings its own copy buttons: a code
  // block's or a quote's copies its text.
  const code = t.closest<HTMLElement>(".dcoderen.agent [data-copy]");
  if (code) return copyCode(code);

  const img = t.closest<HTMLImageElement>(".dcoderen.agent img");

  if (img && !img.closest("a")) {
    return openViewer([{ url: img.currentSrc || img.src, caption: img.alt }], 0);
  }

  const a = t.closest<HTMLAnchorElement>("a[href]");
  if (!a) return;
  e.preventDefault();
  // A link to a file or another session, not a web address: the session view opens it.
  const href = a.getAttribute("href")!;
  if (linkedPath(href) || linkedSession(href)) return;
  invoke("open_url", { url: a.href }).catch(() => {});
});

