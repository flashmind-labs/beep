// Paths mentioned in a coding agent session's next message, and files dropped
// or pasted on the session: they go with the next message, which uploads them
// into its host's temporary cache and mentions them by absolute path.

import { attachPreview, staysInline } from "./images.tsx";
import { AG } from "./agents-state.ts";
import type { Attached } from "./agentqueue.ts";
import { conversationKey } from "./conversation-state.ts";
import { agentMessage } from "./session-frame-state.ts";
import { joinPath, uploadParts, type Transfer } from "../agentpane/files-state.ts";
import { fileBase64, type PastedFiles } from "../chat/attach.tsx";
import { reloadFolder } from "../agentpane/files.ts";
import { paneSession } from "../agentpane/media.ts";
import { mentioned } from "../agentpane/worktree-state.ts";
import { invoke } from "../ui/tauri.ts";
import type { StashFile } from "../chat/stash-state.ts";

/// A file on its way to the session's machine.
interface Upload extends Transfer {
  name: string;
}

/// One attachment batch: files being read, or uploaded to a folder; `size`
/// and `done` count the bytes of all its files.
interface UploadBatch {
  /// Files only read from the disk, which go with the message: its send waits for them.
  local: boolean;
  /// What a local batch attached: the keys of its files, and its pictures.
  paths: string[];
  images: Attached[];
  ready: Promise<void>;
  size: number;
  done: number;
}

/// Files dropped on a folder, waiting for the user to say where they go.
interface UploadAsk {
  files: File[];
  folder: string;
  root: string;
}

export interface MentionState {
  paths: string[];
  uploads: Upload[];
  pending: Set<UploadBatch>;
  ask: UploadAsk | null;
  files: Map<string, StashFile>;
  reading: number;
  images: Attached[];
  error: string;
}

/// How long a failed upload stays said, as the field shows it (agentAttachError).
const ERROR_SHOWN_MS = 5000;
const freshMentions = (): MentionState => ({ paths: [], uploads: [], pending: new Set(), ask: null, files: new Map(), reading: 0, images: [], error: "" });
const conversations = new Map<string, MentionState>();
let current = freshMentions();
export const currentMentions = () => current;
export const mentionImages = (state: MentionState) => state === current ? AG.att : state.images;
export function setMentionImages(state: MentionState, images: Attached[]) {
  state.images = images;
  if (state === current) setAttached(images);
}

/// Replaces the images attached to the open conversation's message: the one place `AG.att` is set.
export function setAttached(images: Attached[]) { AG.att = images; }

/// Takes image `i` off the open message, in place.
export function removeAttached(i: number) { return AG.att.splice(i, 1)[0]; }

/// What went wrong attaching, under the field: the one place `AG.attError` is set.
export function setAttachError(text: string) { AG.attError = text; }
export function rememberMentionImages() { current.images = AG.att; }

export function mentionsOf(session: { machine_id: string; agent: string; id: string }) {
  const key = conversationKey(session);
  let state = conversations.get(key);
  if (!state) { state = freshMentions(); conversations.set(key, state); }
  return state;
}

export function activateMentions(session: { machine_id: string; agent: string; id: string }) {
  const state = mentionsOf(session);
  if (state === current) { setAttachError(state.error); return; }
  current = state;
  setAttached(state.images);
  setAttachError(state.error);
}

export const MN = {
  get paths() { return current.paths; },
  set paths(value: MentionState["paths"]) { current.paths = value; },
  get uploads() { return current.uploads; },
  set uploads(value: MentionState["uploads"]) { current.uploads = value; },
  get pending() { return current.pending; },
  set pending(value: MentionState["pending"]) { current.pending = value; },
  get ask() { return current.ask; },
  set ask(value: MentionState["ask"]) { current.ask = value; },
  get files() { return current.files; },
  set files(value: MentionState["files"]) { current.files = value; },
  get reading() { return current.reading; },
  set reading(value: MentionState["reading"]) { current.reading = value; },
  redraw: () => {},
  error: (_text: string) => {},
};

export function resetMentions() { current = freshMentions(); }

function redrawUploads(state: MentionState) {
  if (state === current) MN.redraw();
}

/// Mentions `path` (relative to the session's folder) in the next message.
export function addMention(path: string) {
  const p = path.replace(/^\.\/+/, "").replace(/\/+$/, "");
  if (p && !MN.paths.includes(p)) MN.paths.push(p);
  MN.redraw();
  agentMessage.current?.focus();
}

export function removeMention(index: number) {
  if (index < 0) return;
  MN.paths.splice(index, 1);
  MN.redraw();
  agentMessage.current?.focus();
}

export function answerUpload(choice: "there" | "default" | "cancel") {
  const ask = MN.ask;
  MN.ask = null;
  MN.redraw();
  if (ask && choice !== "cancel") void uploadDropped(ask.files, choice === "there" ? ask.folder : null, ask.root);
}

/// Files dropped on the folder `folder` of the Files tab: asked first.
export function askUpload(files: File[], folder: string, root = paneSession()?.cwd ?? "") {
  if (!files.length) return;
  MN.ask = { files, folder, root };
  MN.redraw();
}

/// The key of a file attached to the next message and not uploaded yet: its
/// path in `MN.paths`, and its entry in `MN.files`.
export const attachmentKey = (name: string) => `attachment:${++attachments}:${name}`;
let attachments = 0;

/// Files dropped or pasted on the session: attached to the next message,
/// which uploads them behind its row, or uploaded to `folder` at once when
/// that is chosen.
export async function uploadDropped(files: PastedFiles, folder: string | null = null, root = paneSession()?.cwd ?? "", preview = false) {
  const state = current, session = paneSession();
  const pending = state.pending;
  const batch: UploadBatch = { local: folder === null, paths: [], images: [], ready: Promise.resolve(), size: 0, done: 0 };
  pending.add(batch);
  const attach = (dropped: File[]) => {
    batch.size = dropped.reduce((n, f) => n + f.size, 0);
    return folder === null ? attachFiles(dropped, preview, batch, state, session) : uploadFiles(dropped, folder, root, batch, state, session);
  };
  const work = Array.isArray(files) ? attach(files) : files.then(attach);
  batch.ready = work.then(() => {});

  try { await work; }
  finally { pending.delete(batch); }
}

/// The largest file a message uploads; the app refuses larger ones too.
export const MAX_ATTACH_BYTES = 512 * 1024 * 1024;

/// The batches of files being read, which a message sent now takes once they are.
export const attachReads = () => [...current.pending].filter(batch => batch.local);

/// Keeps `files` for the next message. A picture that goes inline as it is
/// crosses the link once, in the message; the others upload with it.
async function attachFiles(files: File[], preview: boolean, batch: UploadBatch, state: MentionState, session: ReturnType<typeof paneSession>) {
  state.error = "";

  for (const f of files) {
    try {
      state.reading++;
      if (preview && staysInline(f)) {
        const added = await attachPreview(mentionImages(state), f, null);
        if (added) { batch.images.push(added); continue; }
      }

      if (!session) continue;

      if (f.size > MAX_ATTACH_BYTES) {
        // Not attached: the app refuses it too, and it would only cross the window as one text.
        const message = `${f.name || "That file"} is larger than ${MAX_ATTACH_BYTES / 1024 / 1024} MiB, the most a message uploads.`;
        state.error = message;
        if (state === current) MN.error(message);
        setTimeout(() => { if (state.error === message) state.error = ""; }, ERROR_SHOWN_MS);
        continue;
      }

      const key = attachmentKey(f.name || "file");
      state.files.set(key, { name: f.name || "file", mime: f.type || "application/octet-stream", size: f.size, data: await fileBase64(f) });
      state.paths.push(key);
      batch.paths.push(key);

      if (preview && f.type.startsWith("image/")) {
        const added = await attachPreview(mentionImages(state), f, key);
        if (added) batch.images.push(added);
      }
    } catch {
      const message = `${f.name || "That file"} could not be read; folders do not attach, drop the files in them`;
      state.error = message;
      if (state === current) MN.error(message);
      setTimeout(() => { if (state.error === message) state.error = ""; }, ERROR_SHOWN_MS);
    } finally {
      state.reading--;
      batch.done += f.size;
      redrawUploads(state);
    }
  }
}

async function uploadFiles(files: File[], folder: string, root: string, batch: UploadBatch, state: MentionState, session: ReturnType<typeof paneSession>) {
  state.error = "";

  for (const f of files) {
    const start = batch.done;

    try {
      state.reading++;
      const path = await uploadOne(f, folder, root, session, state, batch);
      if (path && !state.paths.includes(path)) state.paths.push(path);
    } catch (e) {
      const message = `${f.name || "That file"} did not upload: ${String(e instanceof Error ? e.message : e).replace(/^bad request: /, "")}`;
      state.error = message;
      if (state === current) MN.error(message);
      // The field shows it for a few seconds; the conversation forgets it then too, so it does not come back.
      setTimeout(() => { if (state.error === message) state.error = ""; }, ERROR_SHOWN_MS);
    } finally {
      state.reading--;
      batch.done = start + f.size;
    }

    redrawUploads(state);
  }

  if (state === current) reloadFolder(folder);
}

/// Uploads `file` into `folder` of the session's folder, a part at a time;
/// the path it took there, or "" when the session went away.
async function uploadOne(file: File, folder: string, root: string, s = paneSession(), state = current, batch: UploadBatch | null = null) {
  if (!s) return "";
  const up: Upload = { name: file.name || "file", started: Date.now(), size: file.size, done: 0 };
  state.uploads.push(up);
  redrawUploads(state);

  try {
    let path = joinPath(folder, up.name);

    for (const part of uploadParts(file.size)) {
      // A folder dropped from Finder fails to read here.
      let data: string;

      try { data = await fileBase64(file.slice(part.offset, part.offset + part.len)); }
      catch { throw new Error("it could not be read; folders do not upload, drop the files in them"); }

      path = await invoke<string>("agent_tree_upload", {
        machineId: s.machine_id, root, path, offset: part.offset, size: file.size, data,
        attachment: null,
      });
      up.done = part.offset + part.len;
      if (batch) batch.done += part.len;
      redrawUploads(state);
    }

    return mentioned(root, s.cwd, path);
  } finally {
    state.uploads = state.uploads.filter(u => u !== up);
  }
}
