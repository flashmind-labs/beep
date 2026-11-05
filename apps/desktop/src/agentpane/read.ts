// Reading a tab's file from the session's machine, a part at a time, and
// starting a terminal tab.
import { fileSize } from "../chat/media.ts";
import { dropTerm, followTerm, forgetTerm, openTerm, termStatus } from "./terminal.ts";
import { type FilePart, type TreeText, type MediaTab, P, draw, closeTab, leavePane } from "./media.ts";
import { invoke } from "../ui/tauri.ts";

/// The most the pane reads of one file, as the node allows.
export const MAX_FILE = 128 * 1024 * 1024;

function bytesOf(b64: string) {
  const s = atob(b64), out = new Uint8Array(s.length);
  for (let i = 0; i < s.length; i++) out[i] = s.charCodeAt(i);
  return out;
}

/// Reads all of `path` from `machine`, a part at a time; `progress` hears
/// how far it got.
export async function readFile(machine: string, path: string, progress: (done: number, size: number) => void) {
  const parts: Uint8Array[] = [];
  let done = 0, size = 0, mime = "";
  do {
    const part = await invoke<FilePart>("agent_file", { machineId: machine, path, offset: done });
    size = part.size;
    mime = part.mime;
    if (size > MAX_FILE) throw new Error(`It is ${fileSize(size)}; the pane opens files up to ${fileSize(MAX_FILE)}.`);
    const bytes = bytesOf(part.data);
    if (!bytes.length) break;
    parts.push(bytes);
    done += bytes.length;
    progress(done, size);
  } while (done < size);
  const all = new Uint8Array(done);
  let at = 0;
  for (const p of parts) { all.set(p, at); at += p.length; }
  return { bytes: all, mime };
}

/// What an error from reading a file says to the user.
export function readError(e: unknown) {
  const text = String(e instanceof Error ? e.message : e).replace(/^bad request: /, "");
  if (/no longer|not found|gone/i.test(text)) return `File gone, or ${P.session?.machine_name ?? "the machine"} is offline.`;
  if (/unknown op|unknown request|not serve|too old|closes the stream/i.test(text)) return "beep-node there is too old. Update it.";
  return text;
}

/// Reads the start of a text tab's file.
export async function loadText(t: MediaTab) {
  const s = P.session;
  if (!s || !t.rel) return;
  try {
    t.text = await invoke<TreeText>("agent_tree_text", { machineId: s.machine_id, root: t.root ?? s.cwd, path: t.rel });
    t.size = t.text.size;
  } catch (e) {
    t.error = readError(e);
  }
  if (P.tabs.includes(t)) redraw(t);
}

/// Draws tab `t` again, from scratch.
function redraw(t: MediaTab) {
  t.el?.remove();
  t.el = null;
  draw();
}

/// Starts a terminal tab's shell; when it exits the tab goes.
export async function startTerminal(t: MediaTab) {
  const hooks = {
    onEnd: () => {
      if (t.term) forgetTerm(t.term);
      t.term = undefined;
      closeTab(t.id);
    },
    onLink: () => drawTermStatus(t),
    onEscape: leavePane,
  };
  try {
    // One that runs already is followed; a new one starts where the tab was opened.
    const term = await (t.existing ? followTerm(t.existing, hooks) : openTerm({ ...t.at!, name: t.name }, hooks));
    if (!P.tabs.includes(t)) return dropTerm(term);
    t.term = term;
  } catch (e) {
    if (!P.tabs.includes(t)) return;
    t.error = readError(e);
  }
  redraw(t);
}

export function drawTermStatus(t: MediaTab) {
  const meta = t.el?.querySelector(".agpacts .meta");
  if (meta && t.term) meta.textContent = termStatus(t.term, t.term.row.machine_name);
  const again = t.el?.querySelector<HTMLElement>("[data-tattach]");
  if (again && t.term) again.hidden = t.term.link !== "detached";
}

export async function load(t: MediaTab) {
  const s = P.session;
  if (!s || !t.path) return;
  try {
    const { bytes, mime } = await readFile(s.machine_id, t.path, (done, size) => {
      t.done = done;
      t.size = size;
      const bar = t.el?.querySelector<HTMLElement>(".agpload i");
      if (bar) bar.style.width = `${size ? Math.round((done / size) * 100) : 0}%`;
    });
    if (!P.tabs.includes(t)) return;
    t.size = bytes.length;
    t.bytes = bytes.buffer as ArrayBuffer;
    t.url = URL.createObjectURL(new Blob([bytes as BlobPart], { type: mime || "application/octet-stream" }));
  } catch (e) {
    if (!P.tabs.includes(t)) return;
    t.error = readError(e);
  }
  // Drawn again now its bytes are here.
  redraw(t);
}
