import { createElement } from "react";
import { createDocumentScope } from "../ui/document-root.tsx";
import { Viewer, ViewerNotice } from "./viewer-view.tsx";
import { bar } from "../bar/core.ts";
import { invoke } from "../ui/tauri.ts";
import { savedIn } from "../ui/platform.ts";

/// A picture the viewer shows; one it `saves` can be saved and copied.
export interface ViewerPic { url: string; saves?: boolean; caption?: string; name?: string }

export function fileSize(n: number) {
  if (n < 1024) return `${n} B`;
  if (n < 1024 * 1024) return `${Math.round(n / 1024)} KB`;
  return `${(n / 1024 / 1024).toFixed(1)} MB`;
}

/// What Rust says for a picture that is no longer there.
export const GONE = "No longer available";

/// The pictures the viewer goes through, and the one it shows.
export const V: { pics: ViewerPic[]; at: number } = { pics: [], at: 0 };
let viewerHost = bar;
let viewerFocus: HTMLElement | null = null;
/// The viewer and its notes, drawn at the end of the element they cover.
const overlays = createDocumentScope<"viewer" | "notice">();
let viewerOpen = false;

function disposeViewer() {
  viewerOpen = false;
  overlays.remove(["viewer"]);
}

export function openViewer(pics: ViewerPic[], at: number, host = bar) {
  viewerFocus = document.activeElement instanceof HTMLElement && document.activeElement !== document.body ? document.activeElement : null;
  disposeViewer();
  viewerHost = host;
  Object.assign(V, { pics, at });
  drawViewer();
}

function closeViewer() {
  disposeViewer();
  if (viewerFocus?.isConnected) viewerFocus.focus();
  viewerFocus = null;
}

type ViewerAct = "prev" | "next" | "close" | "save" | "copy" | "open";

/// The open picture over the whole bar, with arrows when there are more.
function drawViewer() {
  viewerOpen = true;
  overlays.set([{ slot: "viewer", host: viewerHost, content: createElement(Viewer, { pictures: V.pics, at: V.at }) }]);
}

export function viewerAct(a: ViewerAct) {
  const p = V.pics[V.at];

  if (a === "prev" || a === "next") {
    const at = V.at + (a === "next" ? 1 : -1);
    if (at >= 0 && at < V.pics.length) { V.at = at; drawViewer(); }
  } else if (a === "close") closeViewer();
  else if (a === "save" && p.saves) saveUrl(p.url, p.name || "image");
  else if (a === "copy" && p.saves) copyUrl(p.url);
  else if (a === "open") invoke("open_url", { url: p.url }).catch(() => {});
}

export function viewerKeys(e: KeyboardEvent, cmd: boolean) {
  const key = e.key.toLowerCase();
  const act = e.key === "Escape" ? "close" : e.key === "ArrowLeft" ? "prev" : e.key === "ArrowRight" ? "next"
    : cmd && key === "s" ? "save" : cmd && key === "c" ? "copy" : null;

  if (act) { e.preventDefault(); viewerAct(act); }
  else if (cmd && key === "q") invoke("quit");
}

/// Saves a picture held as a data URL to the save folder and says where it went.
export async function saveUrl(url: string, name: string) {
  try {
    const path = await invoke<string>("save_picture", { name, url });
    notice(`Saved ${path.split("/").pop()} to ${savedIn(path)}`, { path });
  } catch (e) {
    notice(`Could not save it. ${e}`, { bad: true });
  }
}

/// Copies the picture at `url` as PNG.
async function copyUrl(url: string) {
  const png = (async () => {
    const img = new Image();
    img.src = url;
    await img.decode();
    const canvas = document.createElement("canvas");
    canvas.width = img.naturalWidth;
    canvas.height = img.naturalHeight;
    canvas.getContext("2d")!.drawImage(img, 0, 0);
    return new Promise<Blob>(done => canvas.toBlob(done as BlobCallback, "image/png"));
  })();

  try {
    // Handing over the promise keeps the click's permission to write.
    await navigator.clipboard.write([new ClipboardItem({ "image/png": png })]);
    notice("Copied the picture");
  } catch {
    notice("Could not copy the picture", { bad: true });
  }
}

let noticeTimer: ReturnType<typeof setTimeout> | undefined;
/// A short note at the bottom of the bar, or over the open viewer; one that names a saved file
/// can show it in Finder.
export function notice(text: string, { path, bad }: { path?: string; bad?: boolean } = {}) {
  clearTimeout(noticeTimer);
  overlays.remove(["notice"]);
  overlays.set([{ slot: "notice", host: viewerOpen ? viewerHost : bar, content: createElement(ViewerNotice, { text, path: path ?? null, bad: !!bad }) }]);
  noticeTimer = setTimeout(() => overlays.remove(["notice"]), 5000);
}
