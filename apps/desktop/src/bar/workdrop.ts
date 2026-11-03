// Paths mentioned in a coding agent session's next message, and files dropped
// or pasted on the session: pictures attach as before, other files upload into
// the session's folder on its machine and are mentioned.

import { MAX_UPLOAD, UPLOAD_FOLDER, joinPath, mentionText, shortSize, uploadParts, uploadRefusal } from "../files-state.ts";
import { fileBase64 } from "./attach.ts";
import { esc, ICON, invoke } from "./core.ts";
import { reloadFolder } from "./filespane.ts";
import { paneSession } from "./mediapane.ts";

/// A file on its way to the session's machine.
interface Upload {
  name: string;
  size: number;
  done: number;
}

/// Files dropped on a folder, waiting for the user to say where they go.
interface UploadAsk {
  files: File[];
  folder: string;
}

export const MN = {
  /// Paths relative to the session's folder, as they go in the message.
  paths: [] as string[],
  uploads: [] as Upload[],
  ask: null as UploadAsk | null,
  /// Draws the chips again; the session view sets it.
  redraw: () => {},
  /// Shows a short error by the chips; the session view sets it.
  error: (_text: string) => {},
};

/// Clears what waits for the next message, as when the session changes.
export function resetMentions() {
  MN.paths = [];
  MN.uploads = [];
  MN.ask = null;
}

/// Mentions `path` (relative to the session's folder) in the next message.
export function addMention(path: string) {
  const p = path.replace(/^\.\/+/, "").replace(/\/+$/, "");
  if (p && !MN.paths.includes(p)) MN.paths.push(p);
  MN.redraw();
  document.querySelector<HTMLInputElement>("#agmsg")?.focus();
}

/// Takes the mentions for a message being sent.
export function takeMentions() {
  const paths = MN.paths;
  MN.paths = [];
  MN.redraw();
  return paths;
}

const FILE_ICON = '<svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linejoin="round"><path d="M14 3H6v18h12V7z"/><path d="M14 3v4h4"/></svg>';

/// The chips of mentions and uploads, and the upload question, for the
/// row above the field.
export function mentionChips() {
  const chips = MN.paths.map((p, i) =>
    `<span class="agment" title="Goes in the message as ${esc(mentionText([p]))}">${FILE_ICON}<span class="nm">${esc(p)}</span><button aria-label="Remove ${esc(p)}" data-agment="${i}">${ICON.x}</button></span>`);
  const ups = MN.uploads.map(u =>
    `<span class="agment up" title="Uploading to ${esc(paneSession()?.machine_name ?? "the machine")}"><span class="dspin"></span><span class="nm">${esc(u.name)}</span><small>${u.size ? Math.round((u.done / u.size) * 100) : 0}%</small></span>`);
  const a = MN.ask;
  const ask = a ? `<div class="agupask"><span>Upload ${a.files.length === 1 ? `<b>${esc(a.files[0].name)}</b>` : `${a.files.length} files`} (${esc(shortSize(a.files.reduce((n, f) => n + f.size, 0)))}) to ${esc(paneSession()?.machine_name ?? "the machine")}:</span>`
    + `<button class="dbtn pri" data-agup="there">Into ${esc(a.folder ? `${a.folder}/` : "the session's folder")}</button>`
    + `<button class="dbtn" data-agup="default">Into ${UPLOAD_FOLDER}/</button><button class="dbtn quiet" data-agup="cancel">Cancel</button></div>` : "";
  return chips.join("") + ups.join("") + ask;
}

/// Wires the chips `mentionChips` drew in `el`.
export function wireMentionChips(el: HTMLElement) {
  el.querySelectorAll<HTMLElement>("[data-agment]").forEach(b => b.addEventListener("click", () => {
    MN.paths.splice(Number(b.dataset.agment), 1);
    MN.redraw();
    document.querySelector<HTMLInputElement>("#agmsg")?.focus();
  }));
  el.querySelectorAll<HTMLElement>("[data-agup]").forEach(b => b.addEventListener("click", () => {
    const a = MN.ask;
    MN.ask = null;
    MN.redraw();
    if (!a || b.dataset.agup === "cancel") return;
    uploadAll(a.files, b.dataset.agup === "there" ? a.folder : UPLOAD_FOLDER);
  }));
  if (MN.ask) queueMicrotask(() => el.querySelector<HTMLElement>('[data-agup="there"]')?.focus());
}

/// Files dropped or pasted on the session, other than the pictures the
/// agent takes: uploaded into `.beep/uploads/`.
export function uploadDropped(files: File[]) {
  uploadAll(files, UPLOAD_FOLDER);
}

/// Files dropped on the folder `folder` of the Files tab: asked first.
export function askUpload(files: File[], folder: string) {
  if (!files.length) return;
  MN.ask = { files, folder };
  MN.redraw();
}

async function uploadAll(files: File[], folder: string) {
  for (const f of files) {
    const why = uploadRefusal(f);
    if (why) { MN.error(why); continue; }
    try {
      const path = await uploadOne(f, folder);
      if (path && !MN.paths.includes(path)) MN.paths.push(path);
    } catch (e) {
      MN.error(`${f.name || "That file"} did not upload: ${String(e instanceof Error ? e.message : e).replace(/^bad request: /, "")}`);
    }
    MN.redraw();
  }
  reloadFolder(folder);
}

/// Uploads `file` into `folder` of the session's folder, a part at a time;
/// the path it took there, or "" when the session went away.
async function uploadOne(file: File, folder: string) {
  const s = paneSession();
  if (!s) return "";
  if (file.size > MAX_UPLOAD) throw new Error(uploadRefusal(file));
  const up: Upload = { name: file.name || "file", size: file.size, done: 0 };
  MN.uploads.push(up);
  MN.redraw();
  try {
    let path = joinPath(folder, up.name);
    for (const part of uploadParts(file.size)) {
      // A folder dropped from Finder fails to read here.
      let data: string;
      try { data = await fileBase64(file.slice(part.offset, part.offset + part.len)); }
      catch { throw new Error("it could not be read; folders do not upload, drop the files in them"); }
      path = await invoke<string>("agent_tree_upload", {
        machineId: s.machine_id, root: s.cwd, path, offset: part.offset, size: file.size, data,
      });
      if (paneSession() !== s) return "";
      up.done = part.offset + part.len;
      MN.redraw();
    }
    return path;
  } finally {
    MN.uploads = MN.uploads.filter(u => u !== up);
  }
}
