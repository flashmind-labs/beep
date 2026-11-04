import { hostOf } from "../settings/apps.ts";
import { IMAGE_TYPES } from "./compose-text.ts";
import { focusAsk } from "./chat.ts";
import { $, bar, COPY_ICON, esc, h, ICON, invoke } from "../bar/core.ts";
import type { Turn } from "../bar/core.ts";
import { hints } from "../bar/hints.ts";

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

/// What Beep sent with its reply: files and product cards, under the text
/// and above the suggested replies.
export function sentBox(turn: Turn) {
  let box = turn.querySelector<HTMLElement>(".files");
  if (!box) {
    box = h('<div class="files" style="display:grid;gap:6px"></div>');
    const after = turn.querySelector(".dreplies");
    after ? turn.insertBefore(box, after) : turn.append(box);
  }
  return box;
}

/// A `show_products` row, built from escaped fields on the Rust side. The
/// bar's click handler opens and copies its links.
export function productRow(turn: Turn, html: string) {
  sentBox(turn).append(h(html));
}

/// A file Beep sent: a picture when it is an image, a card otherwise.
export function sentFile(turn: Turn, file: SentFile) {
  const mime = picMime(file);
  mime ? replyPic(turn, { file: { ...file, mime }, caption: "" }) : fileCard(turn, file);
}

/// The picture type of `file`, by its MIME type or else its name, or null.
export function picMime(file: SentFile) {
  if (Object.values(IMAGE_TYPES).includes(file.mime)) return file.mime;
  const ext = file.name.slice(file.name.lastIndexOf(".") + 1).toLowerCase();
  return file.mime === "application/octet-stream" || !file.mime ? IMAGE_TYPES[ext] ?? null : null;
}

/// A file Beep sent. Opening it downloads it once, then hands it to its app.
export function fileCard(turn: Turn, file: SentFile) {
  const box = sentBox(turn);
  const about = [fileKind(file), fileSize(file.size)].filter(Boolean).join(", ");
  const card = h(`<button class="dfile"><span class="ic">${ICON.doc}</span><span style="flex:1;min-width:0;display:grid"><span class="nm">${esc(file.name)}</span><span class="sz">${esc(about)}</span></span><span class="go" style="color:var(--muted);display:grid">${ICON.chev}</span></button>`);
  const sz = card.querySelector(".sz")!, go = card.querySelector(".go")!;
  let busy = false;
  card.addEventListener("click", async () => {
    if (busy || card.classList.contains("gone")) return;
    busy = true;
    sz.textContent = "Downloading…";
    go.innerHTML = '<span class="dspin"></span>';
    try {
      await invoke("open_file", { id: file.id, name: file.name });
      sz.textContent = about;
      go.innerHTML = ICON.chev;
    } catch (e) {
      const gone = String(e) === GONE;
      sz.textContent = gone ? String(e) : `${about}. ${e}`;
      if (gone) { card.classList.add("gone"); go.remove(); } else go.innerHTML = ICON.chev;
    }
    busy = false;
  });
  box.append(card);
}

// ---------- Pictures ----------

/// What Rust says for a file or picture the server no longer has.
export const GONE = "No longer available";
export const GONE_PIC = "Picture no longer available";

/// Pictures people sent, by id: a data URL once fetched, `null` once the
/// server said it no longer has it, a promise while it loads.
export const pictures = new Map<string, string | null | Promise<string | null>>();

/// The pictures a message carried, as a row of tiles under its text.
export function picRow(images: PicRef[] | undefined) {
  if (!images?.length) return "";
  return `<div class="dpics">${images.map(picTile).join("")}</div>`;
}

export function picTile(p: PicRef) {
  if ("url" in p) return `<img class="dpic" src="${esc(p.url)}" alt="">`;
  const got = pictures.get(p.id);
  if (typeof got === "string") return `<img class="dpic" src="${got}" alt="">`;
  if (got === null) return goneTile();
  return `<span class="dpic wait" data-id="${esc(p.id)}" data-mime="${esc(p.mime)}"></span>`;
}

export function goneTile() { return `<span class="dpic gone">${GONE_PIC}</span>`; }

/// Fetches the pictures still loading under `root` and swaps each in.
export function fillPics(root: Element) {
  for (const el of root.querySelectorAll<HTMLElement>(".dpic.wait")) {
    loadPic(el.dataset.id!, el.dataset.mime!).then(url => {
      if (el.isConnected) el.replaceWith(h(url ? `<img class="dpic" src="${url}" alt="">` : goneTile()));
    });
  }
}

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

// ---------- Pictures Beep sends ----------

/// The row of thumbnails under a reply, which lists its pictures for the viewer.
interface PicsRow extends HTMLElement {
  pics: SentImage[];
}

export function picsRow(turn: Turn) {
  const box = sentBox(turn);
  let row = box.querySelector<PicsRow>(".drpics");
  if (!row) {
    row = h<PicsRow>('<div class="drpics"></div>');
    row.pics = [];
    box.prepend(row);
  }
  return row;
}

/// One picture from `send_image` or an image file: `{ url }` from the web or
/// `{ file }` kept with the chat. Hover shows Save, a click opens the viewer.
export function replyPic(turn: Turn, pic: SentImage) {
  const row = picsRow(turn), i = row.pics.push(pic) - 1;
  const name = pic.file?.name ?? "";
  const tile = h(`<span class="drpic wait"><button class="pic" aria-label="${esc(pic.caption || name || "Picture")}"></button>${pic.file ? `<button class="save" aria-label="Save ${esc(name)}" title="Save to Downloads">${ICON.save}</button>` : ""}</span>`);
  const button = tile.querySelector(".pic")!;
  const gone = () => { if (tile.isConnected) tile.replaceWith(h(`<span class="drpic gone">${GONE_PIC}</span>`)); };
  const show = (src: string | null) => {
    if (!src) return gone();
    const img = h<HTMLImageElement>('<img alt="">');
    img.onload = () => tile.classList.remove("wait");
    img.onerror = gone;
    img.src = src;
    button.append(img);
  };
  button.addEventListener("click", () => openViewer(row.pics, i));
  const file = pic.file;
  if (file) tile.querySelector(".save")!.addEventListener("click", () => savePic(file));
  row.append(tile);
  file ? loadPic(file.id, file.mime).then(show) : show(pic.url!);
}

/// The pictures the viewer goes through, and the one it shows.
export const V: { pics: ViewerPic[]; at: number } = { pics: [], at: 0 };

export function openViewer(pics: ViewerPic[], at: number) {
  Object.assign(V, { pics, at });
  $("#keys")?.remove();
  drawViewer();
}

export function closeViewer() {
  $("#viewer")?.remove();
  focusAsk();
}

type ViewerAct = "prev" | "next" | "close" | "save" | "copy" | "open";

/// The open picture over the whole bar, with arrows when there are more.
export function drawViewer() {
  const p = V.pics[V.at], many = V.pics.length > 1;
  const name = p.file ? p.file.name : p.name || hostOf(p.url);
  const about = [many ? `${V.at + 1} of ${V.pics.length}` : "", p.file ? [fileKind(p.file), p.file.size ? fileSize(p.file.size) : ""].filter(Boolean).join(", ") : ""].filter(Boolean).join(" · ");
  const keeps = !!p.file || !!p.saves;
  const acts = keeps
    ? `<button class="dbtn" data-v="copy">${COPY_ICON}Copy<kbd>⌘C</kbd></button><button class="dbtn pri" data-v="save">${ICON.save}Save<kbd>⌘S</kbd></button>`
    : p.url.startsWith("data:") ? "" : `<button class="dbtn" data-v="open">Open in browser</button>`;
  const nav = (dir: "prev" | "next", on: boolean) => many ? `<button class="nav${dir === "prev" ? " prev" : ""}" data-v="${dir}" aria-label="${dir === "prev" ? "Previous" : "Next"}"${on ? "" : " disabled"}>${ICON.chev}</button>` : "";
  const keys = [...(many ? [["← →", "move"]] : []), ...(keeps ? [["⌘S", "save"], ["⌘C", "copy"]] : []), ["esc", "close"]];
  const el = h(`<div class="dover dviewer" id="viewer" role="dialog" aria-label="${esc(name)}">
    <div class="vt"><span class="tt"><b>${esc(name)}</b><small>${esc(about)}</small></span>${acts}<button class="dbtn" data-v="close" aria-label="Close">${ICON.x}</button></div>
    <div class="vi">${nav("prev", V.at > 0)}<img alt="${esc(p.caption)}">${nav("next", V.at < V.pics.length - 1)}</div>
    <div class="cap">${esc(p.caption)}</div>${hints(keys)}</div>`);
  const img = el.querySelector("img")!;
  const gone = () => img.replaceWith(h(`<div class="dmuted" style="flex:1;justify-content:center">${GONE_PIC}</div>`));
  img.onerror = gone;
  if (p.file) loadPic(p.file.id, p.file.mime).then(url => url ? (img.src = url) : gone());
  else img.src = p.url;
  el.addEventListener("click", e => { const b = (e.target as Element).closest<HTMLElement>("[data-v]"); if (b) viewerAct(b.dataset.v as ViewerAct); });
  const old = $("#viewer");
  old ? old.replaceWith(el) : bar.append(el);
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
/// A short note at the bottom of the bar; one that names a saved file can
/// show it in Finder.
export function notice(text: string, { path, bad }: { path?: string; bad?: boolean } = {}) {
  $("#saved")?.remove();
  const el = h(`<div class="dsaved${bad ? " bad" : ""}" id="saved" role="status"><span>${esc(text)}</span>${path ? '<button class="dbtn">Show in Finder</button>' : ""}</div>`);
  el.querySelector("button")?.addEventListener("click", () => invoke("reveal_file", { path }).catch(() => {}));
  bar.append(el);
  clearTimeout(noticeTimer);
  noticeTimer = setTimeout(() => el.remove(), 5000);
}
