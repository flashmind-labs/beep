// One persistent sandbox renders diagrams while streamed replies replace their HTML.
// Previews receive only the cached SVG, so they do not load Mermaid again.
import { type SyntheticEvent } from "react";
import { createDocumentScope } from "../ui/document-root.tsx";
import { skinChanged } from "../ui/skin.ts";
import { invoke, useBuildCapabilities } from "../ui/tauri.ts";
import { saveUrl } from "./media.tsx";
import { notice } from "./viewer-overlay.tsx";

interface Diagram {
  frame: HTMLIFrameElement;
  source: string;
  expanded: boolean;
  worker?: boolean;
  fallback?: HTMLElement;
  timer?: ReturnType<typeof setTimeout>;
}
interface Rendered { svg?: string; failed?: boolean }
const diagrams = new Map<HTMLIFrameElement, Diagram>();
const cache = new Map<string, Rendered>();
const waiting = new Map<string, Set<HTMLElement>>();
let worker: Diagram | null = null;
let working = false;
const dialogScope = createDocumentScope<"dialog">();
let dialog: HTMLDialogElement | null = null;
let returnFocus: HTMLElement | null = null;

function close() {
  if (!dialog) return;
  dialog.close(); dialogScope.remove(["dialog"]); dialog = null;
  returnFocus?.focus(); returnFocus = null;
}
function expand(source: string) {
  if (dialog) close();
  returnFocus = document.activeElement instanceof HTMLElement ? document.activeElement : null;
  dialogScope.set([{ slot: "dialog", host: document.body, content: <dialog
    aria-label="Diagram"
    ref={element => { dialog = element; }}
    onCancel={(event: SyntheticEvent<HTMLDialogElement>) => { event.preventDefault(); close(); }}
    style={{ width: "100vw", height: "100vh", maxWidth: "100vw", maxHeight: "100vh", margin: 0, padding: 0, border: 0, background: "var(--d-bg, #181818)" }} /> }]);
  if (!dialog) return;
  const frame = makeFrame(source, true);
  frame.style.height = "100%";
  dialog.append(frame);
  dialog.showModal(); frame.focus();
}
function render(diagram: Diagram) {
  diagram.frame.contentWindow?.postMessage({ kind: "beep-diagram", action: "render", source: diagram.source,
    expanded: diagram.expanded, inset: diagram.expanded ? chromeInset() : 0, desktop: true, dark: getComputedStyle(document.documentElement).colorScheme === "dark",
    palette: palette(), ...(diagram.worker ? {} : cache.get(diagram.source)) }, "*");
}
/// How far the window's own buttons reach in from the left (--chrome-inset), which the
/// expanded diagram's title row keeps clear of.
function chromeInset() {
  return parseFloat(getComputedStyle(document.documentElement).getPropertyValue("--chrome-inset")) || 0;
}
/// The skin's colors as hex, since the renderer bakes them into the SVG. Cached drawings
/// in another skin's colors are dropped when the skin changes.
interface Palette { bg: string; fg: string; muted: string; box: string; line: string; accent: string }
let lastPalette = "";
function palette(): Palette {
  const probe = document.createElement("i");
  const context = document.createElement("canvas").getContext("2d");
  document.documentElement.append(probe);
  const hex = (name: string) => {
    probe.style.color = `var(${name})`;
    if (!context) return "";
    context.fillStyle = "#000000"; context.fillStyle = getComputedStyle(probe).color;
    return context.fillStyle;
  };
  const colors: Palette = { bg: hex("--surface"), fg: hex("--fg"), muted: hex("--muted"), box: hex("--elev"), line: hex("--border-hi"), accent: hex("--accent") };
  probe.remove();

  const key = JSON.stringify(colors);
  if (key !== lastPalette) { lastPalette = key; cache.clear(); }

  return colors;
}
function makeFrame(source: string, expanded: boolean, fallback?: HTMLElement, isWorker = false) {
  const frame = document.createElement("iframe");
  frame.src = "diagrams/index.html";
  frame.title = isWorker ? "Diagram renderer" : "Mermaid diagram";
  frame.setAttribute("sandbox", "allow-scripts");
  frame.setAttribute("referrerpolicy", "no-referrer");
  Object.assign(frame.style, { width: "100%", height: "320px", display: "block", border: "0" });
  const diagram: Diagram = { frame, source, expanded, fallback, worker: isWorker };

  if (isWorker) {
    frame.setAttribute("aria-hidden", "true"); frame.tabIndex = -1;
    Object.assign(frame.style, { position: "fixed", left: "-10000px", width: "800px", height: "600px", pointerEvents: "none" });
    worker = diagram;
  }

  frame.addEventListener("load", () => {
    render(diagram);
    diagram.timer = setTimeout(() => {
      if (!frame.isConnected) return;

      if (isWorker) finish({ failed: true }, true);
      else if (fallback && !fallback.hidden) {
        const error = document.createElement("p"); error.textContent = "Diagram unavailable. Source is shown below.";
        frame.replaceWith(error);
      }
    }, 20000);
  });
  diagrams.set(frame, diagram);
  return frame;
}
function next() {
  if (working) return;

  for (const [source, elements] of waiting) {
    for (const element of elements) if (!element.isConnected) {
      elements.delete(element);
      delete element.dataset.diagram;
    }
    if (!elements.size) { waiting.delete(source); continue; }
    working = true;

    if (!worker) document.body.append(makeFrame(source, false, undefined, true));
    else {
      worker.source = source; render(worker);
      worker.timer = setTimeout(() => finish({ failed: true }, true), 20000);
    }

    break;
  }
}
function finish(result: Rendered, discard = false) {
  if (!worker || !working) return;
  clearTimeout(worker.timer);
  const source = worker.source;
  if (discard) { worker.frame.remove(); diagrams.delete(worker.frame); worker = null; }
  cache.set(source, result);
  if (cache.size > 16) cache.delete(cache.keys().next().value!);
  for (const pre of waiting.get(source) ?? []) {
    if (pre.isConnected) pre.before(makeFrame(source, false, pre));
    else delete pre.dataset.diagram;
  }
  waiting.delete(source); working = false; next();
}
addEventListener("message", event => {
  const diagram = [...diagrams.values()].find(d => d.frame.contentWindow === event.source);
  if (!diagram || !diagram.frame.isConnected) return;
  const data = event.data;
  if (data?.kind !== "beep-diagram") return;

  if (diagram.worker) {
    if (data.source !== diagram.source) return;
    if (data.action === "rendered") finish(typeof data.svg === "string" && data.svg.length <= 4000000 ? { svg: data.svg } : { failed: true });
    else if (data.action === "error") finish({ failed: true });
    return;
  }

  if (data.action === "resize" && !diagram.expanded && Number.isFinite(data.height)) {
    diagram.frame.style.height = `${Math.max(120, Math.min(700, data.height))}px`;
  } else if (data.action === "rendered" || data.action === "error") {
    clearTimeout(diagram.timer);
    if (diagram.fallback) diagram.fallback.hidden = true;
  } else if (data.action === "expand") expand(diagram.source);
  else if (data.action === "close") close();
  else if (data.action === "copy") navigator.clipboard.writeText(diagram.source).then(() => notice("Copied diagram source"), () => notice("Could not copy diagram source", { bad: true }));
  else if ((data.action === "save" || data.action === "copy-image") && (data.mime === "image/svg+xml" || data.mime === "image/png")
    && typeof data.data === "string" && data.data.length <= 24000000 && /^[A-Za-z0-9+/]*={0,2}$/.test(data.data)) {
    if (data.action === "copy-image" && data.mime === "image/png") {
      if (!useBuildCapabilities.getState().image_clipboard) { notice("Image copying is not available on this device.", { bad: true }); return; }
      void invoke("copy_diagram_image", { png: data.data }).then(() => notice("Copied diagram image"), () => notice("Could not copy diagram image", { bad: true }));
    } else if (data.action === "save") void saveUrl(`data:${data.mime};base64,${data.data}`, data.mime === "image/png" ? "diagram.png" : "diagram.svg");
  }
});
/// Mermaid blocks on the page. The engine keeps this list current, so a scan
/// costs the blocks there are, not the nodes a redraw adds.
const blocks = document.getElementsByClassName("language-mermaid");

function scan() {
  sweep();

  for (const code of blocks) {
    const pre = code.parentElement;
    if (code.localName !== "code" || !(pre instanceof HTMLElement) || pre.localName !== "pre" || pre.dataset.diagram) continue;
    pre.dataset.diagram = "true";
    const source = code.textContent ?? "";

    if (cache.has(source)) pre.before(makeFrame(source, false, pre));
    else {
      const elements = waiting.get(source) ?? new Set<HTMLElement>();
      elements.add(pre); waiting.set(source, elements);
    }
  }

  next();
}
/// Forgets frames a redraw removed; a block whose frame went shows its source and is found again.
/// A frame whose block a redraw replaced goes too, so the new block's frame is the only one.
function sweep() {
  for (const [frame, diagram] of diagrams) {
    if (diagram.fallback && !diagram.fallback.isConnected) frame.remove();
    if (frame.isConnected) continue;
    clearTimeout(diagram.timer); diagrams.delete(frame);
    if (diagram.fallback) {
      delete diagram.fallback.dataset.diagram;
      diagram.fallback.hidden = false;
    }
  }
}
// A Mermaid block not yet drawn plays a 1 ms animation (style.css) as it shows, so a new block
// costs one event, and the rest of the page's changes cost nothing.
document.addEventListener("animationstart", event => { if (event.animationName === "diagram-found") scan(); });
skinChanged.add(() => {
  if (!diagrams.size) return;
  const before = lastPalette;
  palette();
  if (before && lastPalette === before) return;
  for (const [frame, diagram] of diagrams) {
    if (diagram.expanded) render(diagram);
    else if (diagram.fallback) frame.remove();
  }
  scan();
});
scan();
