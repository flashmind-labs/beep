// Paths mentioned in a coding agent session's next message, and files dropped
// or pasted on the session: pictures attach as before, other files upload into
// its host's temporary cache and are mentioned by absolute path.

import { agentMessage } from "./session-frame.tsx";
import { MAX_UPLOAD, joinPath, uploadParts, uploadRefusal } from "../agentpane/files-state.ts";
import { fileBase64 } from "../chat/attach.ts";
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

/// Files dropped on a folder, waiting for the user to say where they go.
interface UploadAsk {
  files: File[];
  folder: string;
  root: string;
}

export const MN = {
  /// Paths relative to the session's folder, as they go in the message.
  paths: [] as string[],
  uploads: [] as Upload[],
  ask: null as UploadAsk | null,
  files: new Map<string, StashFile>(),
  reading: 0,
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
  MN.files.clear();
}

/// Mentions `path` (relative to the session's folder) in the next message.
export function addMention(path: string) {
  const p = path.replace(/^\.\/+/, "").replace(/\/+$/, "");
  if (p && !MN.paths.includes(p)) MN.paths.push(p);
  MN.redraw();
  agentMessage.current?.focus();
}

/// Takes the mentions for a message being sent.
export function takeMentions() {
  const paths = MN.paths;
  MN.paths = [];
  MN.files.clear();
  MN.redraw();
  return paths;
}

export function removeMention(index: number) {
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

/// Files dropped or pasted on the session, other than the pictures the
/// agent takes: temporary attachments by default, permanent when `folder` is chosen.
export async function uploadDropped(files: File[], folder: string | null = null, root = paneSession()?.cwd ?? "") {
  const session = paneSession();

  for (const f of files) {
    if (paneSession() !== session) return;
    const why = uploadRefusal(f);
    if (why) { MN.error(why); continue; }

    try {
      MN.reading++;
      const path = await uploadOne(f, folder, root);
      const data = path ? await fileBase64(f) : "";

      if (path && paneSession() === session) {
        MN.files.set(path, { name: f.name, mime: f.type || "application/octet-stream", size: f.size, data });
        if (!MN.paths.includes(path)) MN.paths.push(path);
      }
    } catch (e) {
      MN.error(`${f.name || "That file"} did not upload: ${String(e instanceof Error ? e.message : e).replace(/^bad request: /, "")}`);
    } finally {
      MN.reading--;
    }

    MN.redraw();
  }

  if (folder !== null) reloadFolder(folder);
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
async function uploadOne(file: File, folder: string | null, root: string) {
  const s = paneSession();
  if (!s) return "";
  if (file.size > MAX_UPLOAD) throw new Error(uploadRefusal(file));
  const up: Upload = { name: file.name || "file", size: file.size, done: 0 };
  MN.uploads.push(up);
  MN.redraw();

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
      if (paneSession() !== s) return "";
      up.done = part.offset + part.len;
      MN.redraw();
    }

    return folder === null ? path : mentioned(root, s.cwd, path);
  } finally {
    MN.uploads = MN.uploads.filter(u => u !== up);
  }
}
