// Paths mentioned in a coding agent session's next message, and files dropped
// or pasted on the session: they upload into its host's temporary cache and
// are mentioned by absolute path.

import { attachPreview } from "./images.tsx";
import { AG } from "./agents.ts";
import type { Attached } from "./agentqueue.ts";
import { conversationKey } from "./conversation-state.ts";
import { agentMessage } from "./session-frame.tsx";
import { joinPath, uploadParts } from "../agentpane/files-state.ts";
import { fileBase64 } from "../chat/attach.tsx";
import { rememberLocalUpload } from "../agentpane/read.ts";
import { reloadFolder } from "../agentpane/files.ts";
import { paneSession } from "../agentpane/media.ts";
import { mentioned } from "../agentpane/worktree-state.ts";
import { invoke } from "../ui/tauri.ts";
import { stashFile, type StashFile } from "../chat/stash-state.ts";

/// A file on its way to the session's machine.
interface Upload {
  name: string;
  size: number;
  done: number;
}

/// One attachment batch, including its image previews; `size` and `done`
/// count the bytes of all its files.
interface UploadBatch {
  paths: string[];
  failed: boolean;
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
  sending: boolean;
  error: string;
}

/// How long a failed upload stays said, as the field shows it (agentAttachError).
const ERROR_SHOWN_MS = 5000;
const freshMentions = (): MentionState => ({ paths: [], uploads: [], pending: new Set(), ask: null, files: new Map(), reading: 0, images: [], sending: false, error: "" });
const conversations = new Map<string, MentionState>();
let current = freshMentions();
export const currentMentions = () => current;
export const mentionImages = (state: MentionState) => state === current ? AG.att : state.images;
export function setMentionImages(state: MentionState, images: Attached[]) {
  state.images = images;
  if (state === current) AG.att = images;
}
export function rememberMentionImages() { current.images = AG.att; }

export function activateMentions(session: { machine_id: string; agent: string; id: string }) {
  const key = conversationKey(session);
  let state = conversations.get(key);
  if (!state) { state = freshMentions(); conversations.set(key, state); }
  if (state === current) { AG.attError = state.error; return; }
  current = state;
  AG.att = state.images;
  AG.attError = state.error;
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

/// Files dropped or pasted on the session: temporary attachments by default,
/// permanent when `folder` is chosen. Gives those that uploaded and where.
export async function uploadDropped(files: File[], folder: string | null = null, root = paneSession()?.cwd ?? "", preview = false) {
  const state = current, session = paneSession();
  const pending = state.pending;
  const batch: UploadBatch = { paths: [], failed: false, ready: Promise.resolve(), size: files.reduce((n, f) => n + f.size, 0), done: 0 };
  pending.add(batch);
  const work = uploadFiles(files, folder, root, preview, batch, state, session);
  batch.ready = work.then(() => {});

  try { return await work; }
  finally { pending.delete(batch); }
}

async function uploadFiles(files: File[], folder: string | null, root: string, preview: boolean, batch: UploadBatch, state: MentionState, session: ReturnType<typeof paneSession>) {
  const uploaded: { file: File; path: string }[] = [];
  state.error = "";

  for (const f of files) {
    const start = batch.done;

    try {
      state.reading++;
      const path = await uploadOne(f, folder, root, session, state, batch);
      const data = path ? await fileBase64(f) : "";

      if (path) {
        if (session) rememberLocalUpload(session.machine_id, path, f);
        state.files.set(path, { name: f.name, mime: f.type || "application/octet-stream", size: f.size, data });
        if (!state.paths.includes(path)) state.paths.push(path);
        uploaded.push({ file: f, path });
        batch.paths.push(path);
        if (preview && f.type.startsWith("image/")) await attachPreview(mentionImages(state), f, path);
      }
    } catch (e) {
      batch.failed = true;
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

  if (folder !== null && state === current) reloadFolder(folder);
  return uploaded;
}

/// Copies stashed files to this session before its stash entry is consumed.
export async function prepareStashFiles(files: StashFile[]) {
  const session = paneSession();
  if (!session && files.length) throw new Error("Open a session before restoring files");
  const restored = new Map<string, StashFile>();

  for (const file of files) {
    const path = await uploadOne(stashFile(file), null, session!.cwd);
    if (!path || paneSession() !== session) throw new Error("The session changed while restoring files");
    restored.set(path, file);
  }

  return restored;
}

/// Uploads `file` into `folder` of the session's folder, a part at a time;
/// the path it took there, or "" when the session went away.
async function uploadOne(file: File, folder: string | null, root: string, s = paneSession(), state = current, batch: UploadBatch | null = null) {
  if (!s) return "";
  const up: Upload = { name: file.name || "file", size: file.size, done: 0 };
  state.uploads.push(up);
  redrawUploads(state);

  try {
    let path = folder === null ? up.name : joinPath(folder, up.name);

    for (const part of uploadParts(file.size)) {
      // A folder dropped from Finder fails to read here.
      let data: string;

      try { data = await fileBase64(file.slice(part.offset, part.offset + part.len)); }
      catch { throw new Error("it could not be read; folders do not upload, drop the files in them"); }

      path = await invoke<string>("agent_tree_upload", {
        machineId: s.machine_id, root, path, offset: part.offset, size: file.size, data,
        attachment: folder === null ? { agent: s.agent, session: s.id } : null,
      });
      up.done = part.offset + part.len;
      if (batch) batch.done += part.len;
      redrawUploads(state);
    }

    return folder === null ? path : mentioned(root, s.cwd, path);
  } finally {
    state.uploads = state.uploads.filter(u => u !== up);
  }
}
