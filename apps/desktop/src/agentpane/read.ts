import { takeQueuedPreview } from "./html-preview-state.ts";
import { togglePreview } from "./text-view.tsx";
// Reading a tab's file from the session's machine, a part at a time, and
// starting a terminal tab.
import { viewKey } from "./view-memory.ts";
import { dropTerm, followTerm, forgetTerm, killTerm, openTerm } from "./terminal.ts";
import { type FilePart, type TreeText, type MediaTab, P, closeTab, leavePane, rememberTerminal } from "./media.ts";
import { draw, touchTab } from "./media-draw.ts";
import { bytesOf } from "./media-state.ts";
import { invoke, listen } from "../ui/tauri.ts";

// Keep the original File on the device that uploaded it. A browser File can
// read its local bytes without downloading the attachment from the node.
const localUploads = new Map<string, File>();
const localUploadKey = (machine: string, path: string) => JSON.stringify([machine, path]);

export function rememberLocalUpload(machine: string, path: string, file: File) {
  const key = localUploadKey(machine, path);
  localUploads.delete(key);
  localUploads.set(key, file);
  if (localUploads.size > 64) {
    const oldest = localUploads.keys().next();
    if (!oldest.done) localUploads.delete(oldest.value);
  }
}

/// How far fetching a file from another machine has come.
type FileProgress = { machine_id: string; path: string; done: number; size: number };

/// Reads all of `path` from `machine`, a part at a time, keeping it for the
/// conversation `session`; `progress` hears how far it got. The first part
/// fetches a file from another machine whole, telling its progress.
export async function readFile(machine: string, path: string, session: string | null, progress: (done: number, size: number) => void) {
  const key = localUploadKey(machine, path), local = localUploads.get(key);
  if (local) {
    try {
      const bytes = new Uint8Array(await local.arrayBuffer());
      progress(bytes.length, bytes.length);
      return { bytes, mime: local.type };
    } catch {
      localUploads.delete(key);
    }
  }

  const parts: Uint8Array[] = [];
  let done = 0, size = 0, mime = "";
  const unlisten = await listen<FileProgress>("agent-file-progress", e => {
    if (e.payload.machine_id === machine && e.payload.path === path) progress(e.payload.done, e.payload.size);
  });

  try {
    do {
      const part = await invoke<FilePart>("agent_file", { machineId: machine, path, offset: done, session });
      size = part.size;
      mime = part.mime;
      const bytes = bytesOf(part.data);
      if (!bytes.length) break;
      parts.push(bytes);
      done += bytes.length;
      progress(done, size);
    } while (done < size);
  } finally {
    unlisten();
  }

  return joined(parts, done, mime);
}

function joined(parts: readonly Uint8Array[], done: number, mime: string) {
  const all = new Uint8Array(done);
  let at = 0;
  for (const p of parts) { all.set(p, at); at += p.length; }
  return { bytes: all, mime };
}

/// What an error from reading a file says to the user.
export function readError(e: unknown) {
  const text = errorText(e);
  if (isMissing(text)) return `Not found on ${machineName()}.`;
  if (/unknown op|unknown request|not serve|too old|closes the stream/i.test(text)) return "beep-node there is too old. Update it.";
  return text;
}

/// What an error from reading the file at `path` says, naming the path when it is missing.
export function fileError(e: unknown, path: string) {
  return isMissing(errorText(e)) ? `${path} not found on ${machineName()}.` : readError(e);
}

function errorText(e: unknown) {
  const said = e instanceof Error ? e.message : typeof e === "object" && e && "message" in e ? e.message : e;
  return String(said).replace(/^bad request: /, "");
}

const isMissing = (text: string) => /no longer|not found|gone/i.test(text);
const machineName = () => P.session?.machine_name ?? "the machine";

/// Reads the start of a text tab's file.
export async function loadText(t: MediaTab) {
  const s = P.session;
  if (!s || !t.rel) return;

  try {
    t.text = await invoke<TreeText>("agent_tree_text", { machineId: s.machine_id, root: t.root ?? s.cwd, path: t.rel });
    t.size = t.text.size;
  } catch (e) {
    if (lookElsewhere(t)) return loadText(t);
    t.error = fileError(e, t.rel);
  }

  if (P.tabs.includes(t)) {
    touchTab(t);
    if (takeQueuedPreview(t) && t.text && !t.text.binary) void togglePreview(t);
  }
}

/// Points `t` at the same file in the session's folder when it was looked
/// for in another worktree; false when there is nowhere else to look.
function lookElsewhere(t: MediaTab) {
  const s = P.session, there = t.elsewhere;
  if (!s || !there) return false;

  t.elsewhere = null;
  const cwd = s.cwd.replace(/\/+$/, "");

  if (t.kind === "text") {
    t.root = s.cwd;
    t.rel = there.startsWith(`${cwd}/`) ? there.slice(cwd.length + 1) : there;
  } else {
    t.path = there;
  }

  return true;
}

/// Starts a terminal tab's shell; when it exits the tab goes.
export async function startTerminal(t: MediaTab) {
  const session = P.session;
  const key = session ? viewKey(session.machine_id, session.id) : null;
  const hooks = {
    onEnd: () => {
      if (t.term) forgetTerm(t.term);
      t.term = undefined;
      if (P.tabs.includes(t)) closeTab(t.id);
    },
    onLink: () => drawTermStatus(t),
    onEscape: leavePane,
  };

  try {
    // One that runs already is followed; a new one starts where the tab was opened.
    const term = await (t.existing ? followTerm(t.existing, hooks) : openTerm({ ...t.at!, name: t.name }, hooks));

    if (key) rememberTerminal(key, t, term.row);

    if (!P.tabs.includes(t)) {
      // Only an explicit tab close stops a shell that was still starting.
      if (t.closed) {
        dropTerm(term);
        return killTerm(term).catch(e => console.warn("stopping a terminal whose tab closed:", e));
      }

      return dropTerm(term);
    }

    t.term = term;
  } catch (e) {
    if (!P.tabs.includes(t)) return;
    t.error = readError(e);
  }

  // Its screen fits the view and takes the keys the pane has.
  t.rev++;
  draw();
}

/// A terminal tab's footer says how its terminal stands again, in place of a note.
export function drawTermStatus(t: MediaTab) {
  t.note = null;
  touchTab(t);
}

export async function load(t: MediaTab) {
  const s = P.session;
  if (!s || !t.path) return;

  try {
    const { bytes, mime } = await readFile(s.machine_id, t.path, s.id, (done, size) => {
      t.done = done;
      t.size = size;
      touchTab(t);
    });
    if (!P.tabs.includes(t)) return;
    t.size = bytes.length;
    t.bytes = bytes.buffer as ArrayBuffer;
    t.url = URL.createObjectURL(new Blob([bytes as BlobPart], { type: mime || "application/octet-stream" }));
  } catch (e) {
    if (!P.tabs.includes(t)) return;
    if (lookElsewhere(t)) return load(t);
    t.error = fileError(e, t.path);
  }

  // Drawn again now its bytes are here.
  touchTab(t);
}
