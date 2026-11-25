import { followFiles } from "./reload.ts";
import { togglePreview } from "./text-view.tsx";
// Reading a tab's file from the session's machine, a part at a time, and
// starting a terminal tab.
import { viewKey } from "./view-memory.ts";
import { dropTerm, followTerm, forgetTerm, killTerm, openTerm } from "./terminal.ts";
import { type FilePart, type TreeText, type MediaTab, P, closeTab, leavePane, newFileEdit, rememberTerminal, openTabs } from "./media.ts";
import { onPhone } from "../ui/platform.ts";
import { draw, touchTab } from "./media-draw.ts";
import { bytesOf, resolvePath } from "./media-state.ts";
import { invoke, listen } from "../ui/tauri.ts";
import { paneShown } from "../ui/active.ts";
import { RETRY_MS } from "../agents/reconnect-state.ts";

/// How far fetching a file from another machine has come.
type FileProgress = { machine_id: string; path: string; done: number; size: number };

/// Reads all of `path` from `machine`, a part at a time; `progress` hears how
/// far it got. A build with blob caching keeps it for `session` and may fetch
/// the whole file before giving the first part.
export async function readFile(machine: string, path: string, session: string | null, progress: (done: number, size: number) => void) {
  const parts: Uint8Array[] = [];
  let done = 0, mime = "";
  let size: number | null = null;
  const unlisten = await listen<FileProgress>("agent-file-progress", e => {
    if (e.payload.machine_id === machine && e.payload.path === path) progress(e.payload.done, e.payload.size);
  });

  try {
    do {
      const part = await invoke<FilePart>("agent_file", { machineId: machine, path, offset: done, session });
      if (!Number.isSafeInteger(part.size) || part.size < 0) throw new Error("The file size is out of range.");
      if (size === null) { size = part.size; mime = part.mime; }
      else if (part.size !== size) throw new Error("The file changed while it was loading. Open it again.");

      const bytes = bytesOf(part.data);
      const next = done + bytes.length;
      if (!Number.isSafeInteger(next)) throw new Error("The file size is out of range.");
      if (next > size || (!bytes.length && done < size)) throw new Error("The file changed while it was loading. Open it again.");

      parts.push(bytes);
      done = next;
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
  if (tooOld(text)) return "beep-node there is too old. Update it.";
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

const isMissing = (text: string) => /no longer|not found|gone|is not there/i.test(text);
const tooOld = (text: string) => /unknown op|unknown request|not serve|too old|closes the stream/i.test(text);
const machineName = () => P.session?.machine_name ?? "the machine";

/// Reads the start of a text tab's file.
export async function loadText(t: MediaTab) {
  const s = P.session;
  if (!s || !t.rel) return;

  try {
    t.text = await invoke<TreeText>("agent_tree_text", { machineId: s.machine_id, root: t.root ?? s.cwd, path: t.rel });
    t.size = t.text.size;
    t.tries = 0;
  } catch (e) {
    if (isMissing(errorText(e)) && lookElsewhere(t)) return loadText(t);
    t.error = fileError(e, t.rel);
    readLater(t, e);
  }

  if (openTabs().includes(t)) {
    if (t.editAt && t.text) {
      const at = t.editAt;
      t.editAt = null;
      if (t.text.editable && !onPhone() && !t.edit) t.edit = newFileEdit(t.text.text, at);
    }
    touchTab(t);
    if (t.previewQueued && t.text) {
      t.previewQueued = false;
      if (!t.text.binary) void togglePreview(t);
    }
  }
}

/// Reads `t` again every `RETRY_MS` while its read fails with an error that
/// may pass, as over a link that dropped, and the pane shows. A reconnect
/// reads it at once.
function readLater(t: MediaTab, e: unknown) {
  const text = errorText(e);
  if (isMissing(text) || tooOld(text)) return;
  t.tries = (t.tries ?? 0) + 1;
  readSoon(t);
}

function readSoon(t: MediaTab) {
  clearTimeout(t.retry);
  t.retry = setTimeout(() => { if (paneShown()) readAgain(t); else readSoon(t); }, RETRY_MS);
}

function readAgain(t: MediaTab) {
  clearTimeout(t.retry);
  if (!openTabs().includes(t) || !t.error || !t.tries) return;
  Object.assign(t, { error: "", started: Date.now(), done: 0 });
  touchTab(t);
  if (t.kind === "text") void loadText(t);
  else void load(t);
}

listen("reconnected", () => {
  for (const t of openTabs()) if (t.error && t.tries) readAgain(t);
});

/// Points `t` at the next place the file may be; false when there is
/// nowhere else to look.
function lookElsewhere(t: MediaTab) {
  const there = t.elsewhere?.shift();
  if (!there) return false;

  if (t.kind === "text") {
    t.root = there.root;
    t.rel = there.rel;
  } else {
    t.path = resolvePath(there.root, there.rel);
  }

  followFiles();
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
      if (openTabs().includes(t)) closeTab(t.id);
    },
    onLink: () => drawTermStatus(t),
    onEscape: leavePane,
  };

  try {
    // One that runs already is followed; a new one starts where the tab was opened.
    const term = await (t.existing ? followTerm(t.existing, hooks) : openTerm({ ...t.at!, name: t.name }, hooks));

    if (key) rememberTerminal(key, t, term.row);

    if (!openTabs().includes(t)) {
      // Only an explicit tab close stops a shell that was still starting.
      if (t.closed) {
        dropTerm(term);
        return killTerm(term).catch(e => console.warn("stopping a terminal whose tab closed:", e));
      }

      return dropTerm(term);
    }

    t.term = term;
  } catch (e) {
    if (!openTabs().includes(t)) return;
    t.error = readError(e);
  }

  // Its screen fits the view and takes the keys the pane has.
  touchTab(t);
  draw();
}

/// A terminal tab's footer says how its terminal stands again, in place of a note.
function drawTermStatus(t: MediaTab) {
  t.note = null;
  touchTab(t);
}

/// Reads a media tab's file. `again` reads it after it changed: what the tab
/// shows stays until the new bytes are here, and stays when they cannot be read.
export async function load(t: MediaTab, again = false) {
  const s = P.session;
  if (!s || !t.path) return;
  if (!again) t.started = Date.now();

  try {
    const { bytes, mime } = await readFile(s.machine_id, t.path, s.id, (done, size) => {
      if (again) return;
      t.done = done;
      t.size = size;
      touchTab(t);
    });
    if (!openTabs().includes(t)) return;
    if (again) replacing(t);
    const was = t.url;
    t.size = bytes.length;
    t.bytes = bytes.buffer as ArrayBuffer;
    t.url = URL.createObjectURL(new Blob([bytes as BlobPart], { type: mime || "application/octet-stream" }));
    if (was?.startsWith("blob:")) URL.revokeObjectURL(was);
    t.tries = 0;
  } catch (e) {
    if (!openTabs().includes(t) || again) return;
    if (isMissing(errorText(e)) && lookElsewhere(t)) return load(t);
    t.error = fileError(e, t.path);
    readLater(t, e);
  }

  // Drawn again now its bytes are here.
  touchTab(t);
}

/// Before a tab takes its file's new bytes: a player goes on from where it
/// stands, playing if it played, and a model is drawn again from them.
function replacing(t: MediaTab) {
  const player = t.player;

  if (player) {
    t.playerAt = player.currentTime;
    if (!player.paused) player.addEventListener("loadedmetadata", () => void player.play().catch(() => {}), { once: true });
  }

  t.model?.dispose();
  t.model = null;
}
