// Reading a file from a machine a part at a time, and what its errors say.
import { invoke, listen } from "../ui/tauri.ts";
import { bytesOf } from "./media-state.ts";
import type { FilePart } from "./media.ts";

/// The machine of the session the pane shows, named in "not found" errors; the pane sets it.
export const readingFrom: { machine: string | null } = { machine: null };

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

export function errorText(e: unknown) {
  const said = e instanceof Error ? e.message : typeof e === "object" && e && "message" in e ? e.message : e;
  return String(said).replace(/^bad request: /, "");
}

export const isMissing = (text: string) => /no longer|not found|gone|is not there/i.test(text);
export const tooOld = (text: string) => /unknown op|unknown request|not serve|too old|closes the stream/i.test(text);
const machineName = () => readingFrom.machine ?? "the machine";
