import { hostOf } from "./apps.ts";
import { focusAsk } from "./chat.ts";
import { $, bar, COPY_ICON, esc, h, ICON, invoke } from "./core.ts";
import { hints } from "./hints.ts";

// ---------- Files ----------

export function fileKind(f) {
  if (f.mime === "application/pdf") return "PDF";
  const dot = f.name.lastIndexOf(".");
  return dot > 0 ? f.name.slice(dot + 1).toUpperCase() : "";
}
export function fileSize(n) {
  if (n < 1024) return `${n} B`;
  if (n < 1024 * 1024) return `${Math.round(n / 1024)} KB`;
  return `${(n / 1024 / 1024).toFixed(1)} MB`;
}

/// What Beep sent with its reply: files and product cards, under the text
/// and above the suggested replies.
export function sentBox(turn) {
  let box = turn.querySelector(".files");
  if (!box) {
    box = h('<div class="files" style="display:grid;gap:6px"></div>');
    const after = turn.querySelector(".dreplies");
    after ? turn.insertBefore(box, after) : turn.append(box);
  }
  return box;
}

/// A `show_products` row, built from escaped fields on the Rust side. The
/// bar's click handler opens and copies its links.
export function productRow(turn, html) {
  sentBox(turn).append(h(html));
}

/// A file Beep sent: a picture when it is an image, a card otherwise.
export function sentFile(turn, file) {
  const mime = picMime(file);
  mime ? replyPic(turn, { file: { ...file, mime }, caption: "" }) : fileCard(turn, file);
}

export const PIC_TYPES = { png: "image/png", jpg: "image/jpeg", jpeg: "image/jpeg", gif: "image/gif", webp: "image/webp" };
/// The picture type of `file`, by its MIME type or else its name, or null.
export function picMime(file) {
  if (Object.values(PIC_TYPES).includes(file.mime)) return file.mime;
  const ext = file.name.slice(file.name.lastIndexOf(".") + 1).toLowerCase();
  return file.mime === "application/octet-stream" || !file.mime ? PIC_TYPES[ext] ?? null : null;
}

/// A file Beep sent. Opening it downloads it once, then hands it to its app.
export function fileCard(turn, file) {
  const box = sentBox(turn);
  const about = [fileKind(file), fileSize(file.size)].filter(Boolean).join(", ");
  const card = h(`<button class="dfile"><span class="ic">${ICON.doc}</span><span style="flex:1;min-width:0;display:grid"><span class="nm">${esc(file.name)}</span><span class="sz">${esc(about)}</span></span><span class="go" style="color:var(--muted);display:grid">${ICON.chev}</span></button>`);
  const sz = card.querySelector(".sz"), go = card.querySelector(".go");
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

/// Pictures people sent, by id: a data URL once fetched, `null` once the
/// server said it no longer has it, a promise while it loads.
export const pictures = new Map();

/// The pictures a message carried, as a row of tiles under its text.
export function picRow(images) {
  if (!images?.length) return "";
  return `<div class="dpics">${images.map(picTile).join("")}</div>`;
}

export function picTile(p) {
  if (p.url) return `<img class="dpic" src="${esc(p.url)}" alt="">`;
  const got = pictures.get(p.id);
  if (typeof got === "string") return `<img class="dpic" src="${got}" alt="">`;
  if (got === null) return goneTile();
  return `<span class="dpic wait" data-id="${esc(p.id)}" data-mime="${esc(p.mime)}"></span>`;
}

export function goneTile() { return '<span class="dpic gone">Picture no longer available</span>'; }

/// Fetches the pictures still loading under `root` and swaps each in.
export function fillPics(root) {
  for (const el of root.querySelectorAll(".dpic.wait")) {
    loadPic(el.dataset.id, el.dataset.mime).then(url => {
      if (el.isConnected) el.replaceWith(h(url ? `<img class="dpic" src="${url}" alt="">` : goneTile()));
    });
  }
}

export function loadPic(id, mime) {
  let got = pictures.get(id);
  if (got === undefined) {
    got = invoke("picture", { id, mime }).then(url => {
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

/// The row of thumbnails under a reply; `pics` lists them for the viewer.
export function picsRow(turn) {
  const box = sentBox(turn);
  let row = box.querySelector(".drpics");
  if (!row) {
    row = h('<div class="drpics"></div>');
    row.pics = [];
    box.prepend(row);
  }
  return row;
}

/// One picture from `send_image` or an image file: `{ url }` from the web or
/// `{ file }` kept with the chat. Hover shows Save, a click opens the viewer.
export function replyPic(turn, pic) {
  const row = picsRow(turn), i = row.pics.push(pic) - 1;
  const name = pic.file?.name ?? "";
  const tile = h(`<span class="drpic wait"><button class="pic" aria-label="${esc(pic.caption || name || "Picture")}"></button>${pic.file ? `<button class="save" aria-label="Save ${esc(name)}" title="Save to Downloads">${ICON.save}</button>` : ""}</span>`);
  const gone = () => { if (tile.isConnected) tile.replaceWith(h(`<span class="drpic gone">${GONE_PIC}</span>`)); };
  const show = src => {
    if (!src) return gone();
    const img = h('<img alt="">');
    img.onload = () => tile.classList.remove("wait");
    img.onerror = gone;
    img.src = src;
    tile.querySelector(".pic").append(img);
  };
  tile.querySelector(".pic").addEventListener("click", () => openViewer(row.pics, i));
  tile.querySelector(".save")?.addEventListener("click", () => savePic(pic.file));
  row.append(tile);
  pic.file ? loadPic(pic.file.id, pic.file.mime).then(show) : show(pic.url);
}

export const GONE_PIC = "Picture no longer available";
export const V = { pics: [], at: 0 };

export function openViewer(pics, at) {
  Object.assign(V, { pics, at });
  $("#keys")?.remove();
  drawViewer();
}

export function closeViewer() {
  $("#viewer")?.remove();
  focusAsk();
}

/// The open picture over the whole bar, with arrows when there are more.
export function drawViewer() {
  const p = V.pics[V.at], many = V.pics.length > 1;
  const name = p.file ? p.file.name : p.name || hostOf(p.url);
  const about = [many ? `${V.at + 1} of ${V.pics.length}` : "", p.file ? [fileKind(p.file), p.file.size ? fileSize(p.file.size) : ""].filter(Boolean).join(", ") : ""].filter(Boolean).join(" · ");
  const acts = p.file
    ? `<button class="dbtn" data-v="copy">${COPY_ICON}Copy<kbd>⌘C</kbd></button><button class="dbtn pri" data-v="save">${ICON.save}Save<kbd>⌘S</kbd></button>`
    : p.url.startsWith("data:") ? "" : `<button class="dbtn" data-v="open">Open in browser</button>`;
  const nav = (dir, on) => many ? `<button class="nav${dir === "prev" ? " prev" : ""}" data-v="${dir}" aria-label="${dir === "prev" ? "Previous" : "Next"}"${on ? "" : " disabled"}>${ICON.chev}</button>` : "";
  const keys = [...(many ? [["← →", "move"]] : []), ...(p.file ? [["⌘S", "save"], ["⌘C", "copy"]] : []), ["esc", "close"]];
  const el = h(`<div class="dover dviewer" id="viewer" role="dialog" aria-label="${esc(name)}">
    <div class="vt"><span class="tt"><b>${esc(name)}</b><small>${esc(about)}</small></span>${acts}<button class="dbtn" data-v="close" aria-label="Close">${ICON.x}</button></div>
    <div class="vi">${nav("prev", V.at > 0)}<img alt="${esc(p.caption)}">${nav("next", V.at < V.pics.length - 1)}</div>
    <div class="cap">${esc(p.caption)}</div>${hints(keys)}</div>`);
  const img = el.querySelector("img");
  const gone = () => img.replaceWith(h(`<div class="dmuted" style="flex:1;justify-content:center">${GONE_PIC}</div>`));
  img.onerror = gone;
  if (p.file) loadPic(p.file.id, p.file.mime).then(url => url ? (img.src = url) : gone());
  else img.src = p.url;
  el.addEventListener("click", e => { const b = e.target.closest("[data-v]"); if (b) viewerAct(b.dataset.v); });
  const old = $("#viewer");
  old ? old.replaceWith(el) : bar.append(el);
}

export function viewerAct(a) {
  const p = V.pics[V.at];
  if (a === "prev" || a === "next") {
    const at = V.at + (a === "next" ? 1 : -1);
    if (at >= 0 && at < V.pics.length) { V.at = at; drawViewer(); }
  } else if (a === "close") closeViewer();
  else if (a === "save" && p.file) savePic(p.file);
  else if (a === "copy" && p.file) copyPic(p.file);
  else if (a === "open" && p.url) invoke("open_url", { url: p.url }).catch(() => {});
}

export function viewerKeys(e, cmd) {
  const key = e.key.toLowerCase();
  const act = e.key === "Escape" ? "close" : e.key === "ArrowLeft" ? "prev" : e.key === "ArrowRight" ? "next"
    : cmd && key === "s" ? "save" : cmd && key === "c" ? "copy" : null;
  if (act) { e.preventDefault(); viewerAct(act); }
  else if (cmd && key === "q") invoke("quit");
}

/// Saves a picture Beep sent to Downloads and says where it went.
export async function savePic(file) {
  try {
    const path = await invoke("save_file", { id: file.id, name: file.name });
    notice(`Saved ${file.name} to Downloads`, { path });
  } catch (e) {
    notice(String(e) === GONE ? GONE_PIC : `Could not save it. ${e}`, { bad: true });
  }
}

/// Copies a picture as PNG, the type every app pastes.
export async function copyPic(file) {
  const url = await loadPic(file.id, file.mime);
  if (!url) return notice(GONE_PIC, { bad: true });
  const png = (async () => {
    const img = new Image();
    img.src = url;
    await img.decode();
    const canvas = document.createElement("canvas");
    canvas.width = img.naturalWidth;
    canvas.height = img.naturalHeight;
    canvas.getContext("2d").drawImage(img, 0, 0);
    return new Promise(done => canvas.toBlob(done, "image/png"));
  })();
  try {
    // Handing over the promise keeps the click's permission to write.
    await navigator.clipboard.write([new ClipboardItem({ "image/png": png })]);
    notice("Copied the picture");
  } catch {
    notice("Could not copy the picture", { bad: true });
  }
}

/// A short note at the bottom of the bar; one that names a saved file can
/// show it in Finder.
export function notice(text, { path, bad } = {}) {
  $("#saved")?.remove();
  const el = h(`<div class="dsaved${bad ? " bad" : ""}" id="saved" role="status"><span>${esc(text)}</span>${path ? '<button class="dbtn">Show in Finder</button>' : ""}</div>`);
  el.querySelector("button")?.addEventListener("click", () => invoke("reveal_file", { path }).catch(() => {}));
  bar.append(el);
  clearTimeout(notice.timer);
  notice.timer = setTimeout(() => el.remove(), 5000);
}

