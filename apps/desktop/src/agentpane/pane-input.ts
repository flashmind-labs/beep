// What the user does to the pane's docks: ⇧esc to zoom one, ⌃tab between tabs,
// the dividers, and what a sandboxed preview sends to its pane. Above the
// commands in media.ts, which it calls.
import { fileBase64 } from "../chat/pics.ts";
import { showFailure } from "../ui/failbanner-view.tsx";
import { invoke } from "../ui/tauri.ts";
import { S } from "../bar/core.ts";
import { type Dock, frameKey, onZoomKey, paneSize } from "./media-state.ts";
import { dockHolding, docked, drawPaneNow, foldNarrow, main, paneModel, paneOf, place, setHidden, shell, shellSize, shownDocks } from "./media-layout.ts";
import { openPreviewExport, showTab } from "./media.ts";
import { rememberPane, saveLayout } from "./pane-memory.ts";
import { P } from "./pane-state.ts";

/// How far an arrow key moves a divider.
const ARROW_STEP = 24;

/// ⇧esc: the dock with focus, else the right one, else the bottom one, fills the session
/// over the conversation; again puts it back. Only classes change, so a model, a video
/// or a terminal keeps its state and fits the new size on its own.
function toggleZoom() {
  // Narrow, the open right pane already covers the conversation: it folds instead.
  if (foldNarrow() && !paneModel.getState().layout.hidden.right) setHidden("right", true);
  else if (paneModel.getState().zoom) paneModel.setState({ zoom: null });
  else {
    const shown = shownDocks();
    paneModel.setState({ zoom: dockHolding(document.activeElement) ?? shown.find(d => d === "right") ?? shown[0] ?? null });
  }

  place();
  drawPaneNow();
  rememberPane();
  // From the input, the zoomed tab takes the keys: a terminal fills the view and is typed in.
  const { zoom, on } = paneModel.getState(), pane = zoom && paneOf(zoom);

  if (zoom && pane && !pane.contains(document.activeElement)) {
    const term = paneModel.getState().tabs.find(t => t.id === on[zoom])?.term;
    if (term) term.screen.focus(); else pane.focus({ preventScroll: true });
  }
}

// S.view stays "agent" while the center shows another page; only a session on screen zooms.
onZoomKey(() => S.view === "agent" && !!P.session && shellSize().width > 0, toggleZoom);

// Sandboxed previews send links, exports and shortcuts to their owning pane.
window.addEventListener("message", (event: MessageEvent<unknown>) => {
  const data = event.data;
  if (!data || typeof data !== "object" || !("beep" in data)) return;

  if (data.beep === "link" || data.beep === "export") {
    const frame = document.activeElement;
    if (!(frame instanceof HTMLIFrameElement) || !frame.matches(".agartframe") || frame.contentWindow !== event.source) return;
    if (event.origin !== "null" || !navigator.userActivation.isActive) return;
    if (data.beep === "export") {
      const port = event.ports[0], dock = dockHolding(frame);
      if (port && dock) acceptPreviewExport(frame, port, dock);
    } else if ("url" in data && typeof data.url === "string" && /^https?:\/\//.test(data.url)) {
      invoke("open_url", { url: data.url }).catch(e => console.warn("opening a preview link:", e));
    }
    return;
  }

  const key = frameKey(data);
  const frame = key && [...document.querySelectorAll<HTMLIFrameElement>("iframe.agartframe")].find(f => f.contentWindow === event.source);
  if (frame) frame.dispatchEvent(new KeyboardEvent("keydown", { ...key, bubbles: true, cancelable: true }));
});

const pendingExports = new WeakSet<HTMLIFrameElement>();
const MAX_EXPORT_BYTES = 16 * 1024 * 1024;

function acceptPreviewExport(frame: HTMLIFrameElement, port: MessagePort, dock: Dock) {
  const session = P.session;
  if (!session || frame.closest("[hidden]") || pendingExports.has(frame)) {
    port.close();
    return;
  }

  const source = { tab: paneModel.getState().on[dock], url: frame.src };
  pendingExports.add(frame);
  const fail = (reason: unknown) => showFailure(`Could not export file.\n\n${reason instanceof Error ? reason.message : String(reason)}`);
  const close = () => { clearTimeout(timeout); port.close(); };
  const timeout = setTimeout(() => {
    close();
    pendingExports.delete(frame);
    fail("The preview stopped sending the export.");
  }, 35_000);
  port.onmessageerror = () => {
    close();
    pendingExports.delete(frame);
    fail("Could not read the export.");
  };
  port.onmessage = (event: MessageEvent<unknown>) => {
    close();
    void savePreviewExport(event.data).then(({ path, blob }) => {
      if (P.session === session && shell()) openPreviewExport(path, blob, source);
    }).catch(fail).finally(() => pendingExports.delete(frame));
  };
}

async function savePreviewExport(data: unknown) {
  if (!data || typeof data !== "object") throw new Error("Could not read the export.");
  if ("error" in data && data.error === "too-large") throw new Error("The export is larger than 16 MB.");
  if (!("name" in data) || typeof data.name !== "string" || !("blob" in data) || !(data.blob instanceof Blob)) {
    throw new Error("Could not read the export.");
  }
  if (data.blob.size > MAX_EXPORT_BYTES) throw new Error("The export is larger than 16 MB.");

  const mime = data.blob.type.split(";")[0];
  const kind = mime === "image/svg+xml" ? "svg" : mime === "application/pdf" ? "pdf" : /\.(svg|pdf)$/i.exec(data.name)?.[1].toLowerCase();
  if (kind !== "svg" && kind !== "pdf") throw new Error("Only SVG and PDF exports are supported.");

  const blob = new Blob([data.blob], { type: kind === "svg" ? "image/svg+xml" : "application/pdf" });
  const path = await invoke<string>("agent_preview_export", { name: data.name.slice(0, 1024), kind, data: await fileBase64(blob) });
  return { path, blob };
}

// ⌃tab with focus in a dock: its next tab, round to the first; ⌃⇧tab the one before.
window.addEventListener("keydown", e => {
  if (e.key !== "Tab" || !e.ctrlKey || e.metaKey || e.altKey) return;
  const d = dockHolding(e.target instanceof Node ? e.target : null);
  if (!d) return;
  e.preventDefault();
  e.stopImmediatePropagation();
  const ids = docked()[d];
  const step = e.shiftKey ? ids.length - 1 : 1;
  showTab(d, ids[(ids.indexOf(paneModel.getState().on[d]) + step) % ids.length]);
}, true);

// ----- Dividers -----

/// Sets dock `dock`'s size, or the default for null, as the shell allows.
function resize(dock: Dock, size: number | null) {
  if (!shell() || !main()) return;
  const { width, height } = shellSize();
  paneModel.setState(s => ({ layout: { ...s.layout, [dock]: size == null ? null : paneSize(dock, size, dock === "right" ? width : height) } }));
  place();
}

/// A drag of dock `dock`'s divider. While it moves only the dock's size, a
/// CSS variable on its element, changes; the layout takes it when the drag ends.
export function dividerDrag(dock: Dock) {
  const el = shell(), box = main(), pane = paneOf(dock);
  if (!el || !box || !pane) return;
  paneModel.setState({ resizing: dock });
  const right = dock === "right";
  const r = (right ? el : box).getBoundingClientRect();
  const total = right ? el.clientWidth : box.clientHeight;
  let size: number | null = null;
  return {
    move: (ev: PointerEvent) => {
      size = paneSize(dock, right ? r.right - ev.clientX : r.bottom - ev.clientY, total);
      pane.style.setProperty("--drag-size", `${size}px`);
    },
    end: () => {
      paneModel.setState({ resizing: null });
      if (size !== null) resize(dock, size);
      pane.style.removeProperty("--drag-size");
      saveLayout();
    },
  };
}

/// Double-click on a divider: the dock's default size.
export function resetDivider(dock: Dock) {
  resize(dock, null);
  saveLayout();
}

/// Arrow keys on a divider grow or shrink its dock; false for other keys.
export function dividerKey(dock: Dock, key: string) {
  const pane = paneOf(dock);
  const grow: Record<string, number> = dock === "right" ? { ArrowLeft: 1, ArrowRight: -1 } : { ArrowUp: 1, ArrowDown: -1 };
  const by = grow[key];
  if (!by || !pane) return false;
  const now = dock === "right" ? pane.offsetWidth : pane.offsetHeight;
  resize(dock, now + by * ARROW_STEP);
  saveLayout();
  return true;
}
