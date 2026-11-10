import { createElement } from "react";
import { createDocumentScope } from "../ui/document-root.tsx";
import { Viewer, ViewerNotice } from "./viewer-view.tsx";
import { IMAGE_TYPES } from "./compose-text.ts";
import { focusAsk } from "./chat.ts";
import { $, bar } from "../bar/core.ts";
import { invoke } from "../ui/tauri.ts";

// ---------- Files ----------

/// A file Beep sent in the chat (`SentFile`), or one a prompt carried that
/// the server keeps (`FileRef`).
export interface SentFile {
  id: string;
  name: string;
  mime: string;
  size: number;
}

/// A picture Beep sent with `send_image` (`SentImage`): an https image on
/// the web, or one kept with the chat.
export type SentImage = ({ url: string; file?: undefined } | { file: SentFile; url?: undefined }) & { caption: string };

/// A picture someone sent, kept on the server (`ImageRef`).
export interface ImageRef {
  id: string;
  mime: string;
  width: number;
  height: number;
}

/// A picture a message carried: one just attached, as a data URL, or one
/// the server keeps.
export type PicRef = { url: string } | ImageRef;

/// A kept picture the viewer can save and copy. An agent's has no size.
export type PicFile = Omit<SentFile, "size"> & { size?: number };

/// A picture the viewer shows.
export type ViewerPic = ({ url: string; file?: undefined; saves?: boolean } | { file: PicFile; url?: undefined; saves?: undefined }) & { caption?: string; name?: string };

/// The kind of a file, as its chip says it: PDF, or its extension.
export function fileKind(f: { name: string; mime?: string }) {
  if (f.mime === "application/pdf") return "PDF";
  const dot = f.name.lastIndexOf(".");
  return dot > 0 ? f.name.slice(dot + 1).toUpperCase() : "";
}
export function fileSize(n: number) {
  if (n < 1024) return `${n} B`;
  if (n < 1024 * 1024) return `${Math.round(n / 1024)} KB`;
  return `${(n / 1024 / 1024).toFixed(1)} MB`;
}

/// A `show_products` card (`ProductCard`), its fields checked by Rust.
export interface ProductCard {
  title: string;
  url: string;
  store: string;
  image: string | null;
  price: string | null;
  was: string | null;
  /// Rating, review count and note, as `4.6 ★ (1,204) · Quietest`.
  meta: string | null;
  label: string;
}

/// What a `show_products` or `show_cards` row shows: typed product cards, or
/// recommendation cards Rust escapes into sanitized markup. The bar's click
/// handler opens and copies their links.
export type SentCards = { kind: "products"; cards: ProductCard[] } | { kind: "html"; html: string };

/// The picture type of `file`, by its MIME type or else its name, or null.
export function picMime(file: SentFile) {
  if (Object.values(IMAGE_TYPES).includes(file.mime)) return file.mime;
  const ext = file.name.slice(file.name.lastIndexOf(".") + 1).toLowerCase();
  return file.mime === "application/octet-stream" || !file.mime ? IMAGE_TYPES[ext] ?? null : null;
}

// ---------- Pictures ----------

/// What Rust says for a file or picture the server no longer has.
export const GONE = "No longer available";
export const GONE_PIC = "Picture no longer available";

/// Pictures people sent, by id: a data URL once fetched, `null` once the
/// server said it no longer has it, a promise while it loads.
export const pictures = new Map<string, string | null | Promise<string | null>>();

export function loadPic(id: string, mime: string) {
  let got = pictures.get(id);

  if (got === undefined) {
    got = invoke<string>("picture", { id, mime }).then(url => {
      pictures.set(id, url);
      return url;
    }, e => {
      // Only a picture the server dropped stays gone; anything else tries again next time.
      if (String(e) === GONE) pictures.set(id, null);
      else pictures.delete(id);
      return null;
    });
    pictures.set(id, got);
  }

  return Promise.resolve(got);
}

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
  $("#keys")?.remove();
  drawViewer();
}

export function closeViewer() {
  disposeViewer();
  if (viewerFocus?.isConnected) viewerFocus.focus(); else focusAsk();
  viewerFocus = null;
}

type ViewerAct = "prev" | "next" | "close" | "save" | "copy" | "open";

/// The open picture over the whole bar, with arrows when there are more.
export function drawViewer() {
  viewerOpen = true;
  overlays.set([{ slot: "viewer", host: viewerHost, content: createElement(Viewer, { pictures: V.pics, at: V.at }) }]);
}

export function viewerAct(a: ViewerAct) {
  const p = V.pics[V.at];

  if (a === "prev" || a === "next") {
    const at = V.at + (a === "next" ? 1 : -1);
    if (at >= 0 && at < V.pics.length) { V.at = at; drawViewer(); }
  } else if (a === "close") closeViewer();
  else if (a === "save" && p.file) savePic(p.file);
  else if (a === "save" && p.saves) saveUrl(p.url, p.name || "image");
  else if (a === "copy" && p.file) copyPic(p.file);
  else if (a === "copy" && p.saves) copyUrl(p.url);
  else if (a === "open" && p.url) invoke("open_url", { url: p.url }).catch(() => {});
}

export function viewerKeys(e: KeyboardEvent, cmd: boolean) {
  const key = e.key.toLowerCase();
  const act = e.key === "Escape" ? "close" : e.key === "ArrowLeft" ? "prev" : e.key === "ArrowRight" ? "next"
    : cmd && key === "s" ? "save" : cmd && key === "c" ? "copy" : null;

  if (act) { e.preventDefault(); viewerAct(act); }
  else if (cmd && key === "q") invoke("quit");
}

/// Saves a picture Beep sent to Downloads and says where it went.
export async function savePic(file: PicFile) {
  try {
    const path = await invoke<string>("save_file", { id: file.id, name: file.name });
    notice(`Saved ${file.name} to Downloads`, { path });
  } catch (e) {
    notice(String(e) === GONE ? GONE_PIC : `Could not save it. ${e}`, { bad: true });
  }
}

/// Saves a picture held as a data URL to Downloads and says where it went.
export async function saveUrl(url: string, name: string) {
  try {
    const path = await invoke<string>("save_picture", { name, url });
    notice(`Saved ${path.split("/").pop()} to Downloads`, { path });
  } catch (e) {
    notice(`Could not save it. ${e}`, { bad: true });
  }
}

/// Copies a picture as PNG, the type every app pastes.
export async function copyPic(file: PicFile) {
  const url = await loadPic(file.id, file.mime);
  if (!url) return notice(GONE_PIC, { bad: true });
  return copyUrl(url);
}

/// Copies the picture at `url` as PNG.
export async function copyUrl(url: string) {
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
