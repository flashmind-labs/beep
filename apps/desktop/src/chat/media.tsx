import { createDocumentScope } from "../ui/document-root.tsx";
import { FileViewer, Viewer, ViewerNotice } from "./viewer-view.tsx";
import { baseName, mediaKind } from "../agentpane/media-state.ts";
import { readError, readFile } from "../agentpane/read.ts";
import type { TreeText } from "../agentpane/media.ts";
import { bar } from "../bar/core.ts";
import { invoke, useBuildCapabilities } from "../ui/tauri.ts";
import { savedIn } from "../ui/platform.ts";

/// A picture the viewer shows; one it `saves` can be saved and copied.
export interface ViewerPic { url: string; saves?: boolean; caption?: string; name?: string }

/// A file on a machine that the viewer shows over the window, and where reading it got.
export interface ViewerFile { machine: string; session: string; path: string; name: string; read: FileRead }
/// A picture, audio or video plays from its bytes and text reads as text. A PDF
/// or 3D model is not read: it opens in this computer's app, as anything can.
type FileRead = { kind: "loading" } | { kind: "media"; media: "image" | "audio" | "video"; url: string } | { kind: "text"; text: string; cut: boolean }
  | { kind: "elsewhere" } | { kind: "failed"; error: string };

export function fileSize(n: number) {
  if (n < 1024) return `${n} B`;
  if (n < 1024 * 1024) return `${Math.round(n / 1024)} KB`;
  return `${(n / 1024 / 1024).toFixed(1)} MB`;
}

/// What Rust says for a picture that is no longer there.
export const GONE = "No longer available";

/// The pictures the viewer goes through, and the one it shows, or the file it shows instead.
const V: { pics: ViewerPic[]; at: number; file: ViewerFile | null } = { pics: [], at: 0, file: null };
let viewerHost = bar;
let viewerFocus: HTMLElement | null = null;
/// The viewer and its notes, drawn at the end of the element they cover.
const overlays = createDocumentScope<"viewer" | "notice">();
let viewerOpen = false;

function disposeViewer() {
  viewerOpen = false;
  if (V.file?.read.kind === "media") URL.revokeObjectURL(V.file.read.url);
  V.file = null;
  overlays.remove(["viewer"]);
}

export function openViewer(pics: ViewerPic[], at: number, host = bar) {
  takeOver(host);
  Object.assign(V, { pics, at });
  drawViewer();
}

/// Replaces what the viewer shows, keeping where the focus goes back to.
function takeOver(host: HTMLElement) {
  viewerFocus = document.activeElement instanceof HTMLElement && document.activeElement !== document.body ? document.activeElement : null;
  disposeViewer();
  viewerHost = host;
}

function closeViewer() {
  disposeViewer();
  if (viewerFocus?.isConnected) viewerFocus.focus();
  viewerFocus = null;
}

export type ViewerAct = "prev" | "next" | "close" | "save" | "copy" | "open";

/// Shows `path`, a file in or under `cwd` on `machine`, over the window while it is read.
export function openFileViewer(machine: string, session: string, cwd: string, path: string) {
  const media = mediaKind(path);
  const elsewhere = media === "pdf" || media === "model";
  const file: ViewerFile = { machine, session, path, name: baseName(path), read: elsewhere ? { kind: "elsewhere" } : { kind: "loading" } };
  takeOver(bar);
  V.file = file;
  drawViewer();
  if (elsewhere) return;

  const read: Promise<FileRead> = media === "image" || media === "audio" || media === "video"
    ? readFile(machine, path, session, () => {}).then(({ bytes, mime }) => ({ kind: "media", media, url: URL.createObjectURL(new Blob([bytes as BlobPart], { type: mime || "application/octet-stream" })) }))
    : readText(machine, cwd, path);

  read
    .catch((e): FileRead => ({ kind: "failed", error: readError(e) }))
    .then(read => {
      if (V.file !== file) return read.kind === "media" && URL.revokeObjectURL(read.url);
      file.read = read;
      drawViewer();
    });
}

/// The start of a text file, as the pane reads it, or opened elsewhere when it is not text.
async function readText(machine: string, cwd: string, path: string): Promise<FileRead> {
  const top = cwd.replace(/\/+$/, "");
  const [root, rel] = path.startsWith(`${top}/`) ? [top, path.slice(top.length + 1)] : [path.slice(0, path.lastIndexOf("/")) || "/", baseName(path)];
  const text = await invoke<TreeText>("agent_tree_text", { machineId: machine, root, path: rel });
  return text.binary ? { kind: "elsewhere" } : { kind: "text", text: text.text, cut: text.cut };
}

/// Opens the file the viewer shows in this computer's app for it.
export function openFileElsewhere(file: ViewerFile) {
  invoke("agent_file_open", { machineId: file.machine, path: file.path, session: file.session }).catch(e => notice(readError(e), { bad: true }));
}

/// The open picture over the whole bar, with arrows when there are more.
function drawViewer() {
  viewerOpen = true;
  const content = V.file ? <FileViewer key={V.file.path} file={{ ...V.file }} act={viewerAct} open={openFileElsewhere} /> : <Viewer pictures={V.pics} at={V.at} act={viewerAct} onKey={viewerKeys} />;
  overlays.set([{ slot: "viewer", host: viewerHost, content }]);
}

export function viewerAct(a: ViewerAct) {
  if (V.file) return void (a === "close" && closeViewer());
  const p = V.pics[V.at];

  if (a === "prev" || a === "next") {
    const at = V.at + (a === "next" ? 1 : -1);
    if (at >= 0 && at < V.pics.length) { V.at = at; drawViewer(); }
  } else if (a === "close") closeViewer();
  else if (a === "save" && p.saves) saveUrl(p.url, p.name || "image");
  else if (a === "copy" && p.saves && useBuildCapabilities.getState().image_clipboard) copyUrl(p.url);
  else if (a === "open") invoke("open_url", { url: p.url }).catch(() => {});
}

export function viewerKeys(e: KeyboardEvent, cmd: boolean) {
  const key = e.key.toLowerCase();
  const act = e.key === "Escape" ? "close" : e.key === "ArrowLeft" ? "prev" : e.key === "ArrowRight" ? "next"
    : cmd && key === "s" ? "save" : cmd && key === "c" && useBuildCapabilities.getState().image_clipboard ? "copy" : null;

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
  overlays.set([{ slot: "notice", host: viewerOpen ? viewerHost : bar, content: <ViewerNotice text={text} path={path ?? null} bad={!!bad} /> }]);
  noticeTimer = setTimeout(() => overlays.remove(["notice"]), 5000);
}
