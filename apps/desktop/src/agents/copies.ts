// The copies this Mac keeps of other machines' sessions, in the Coding
// agents list: a session whose machine is off offers Continue on this Mac,
// and a version that lost to another machine's can be restored or thrown
// away. The node brings the copies in on its own; nothing here starts one.
import { createStore } from "zustand/vanilla";
import { subscribeChanges, topic } from "../ui/mesh-events.ts";
import { type CopyRow, copyKey, copyLists, discardLabel, lostLine, offlineLine } from "./copy-state.ts";
import { loadSessions } from "./sessions.ts";
import { invoke } from "../ui/tauri.ts";
import { pageShown } from "../ui/active.ts";

/// The copies read, the last failure, the copy whose discard asks to be
/// confirmed, and the copies with a request out.
export const copies = createStore(() => ({ rows: [] as CopyRow[], error: "", armed: "", busy: [] as string[] }));

let showing: () => boolean = () => false;
let unfollow: (() => void) | undefined;

/// Follows copy and machine changes while the list shows. Started once.
export function keepCopiesFresh(isOpen: () => boolean) {
  showing = isOpen;
  unfollow ??= subscribeChanges([topic("moves"), topic("machines")], { deliver: "latest", receive: () => loadCopies() }, { when: () => showing() && pageShown() });
}

/// Reads the copies the node keeps.
export async function loadCopies() {
  copies.setState({ rows: await readCopies() });
}

async function readCopies(): Promise<CopyRow[]> {
  try { const rows = await invoke<CopyRow[]>("session_copies"); return Array.isArray(rows) ? rows : []; }
  catch (e) { console.warn("copies not read", e); return []; }
}

export type CopyAct = "continue" | "discard";
export interface CopyLine {
  key: string;
  /// The project, and the folder in it when there is one.
  title: string;
  /// The harness and what happened to the copy.
  sub: string;
  buttons: { act: CopyAct; label: string; pri: boolean; busy: boolean }[];
}
export interface CopiesModel { error: string; rows: CopyLine[] }

/// The rows to draw under the sessions; `live` are the ids of the sessions
/// the machines list. Null when there is nothing to show.
export function copiesView(live: ReadonlySet<string>): CopiesModel | null {
  const { rows, error, armed, busy } = copies.getState();
  const { offline, lost } = copyLists(rows, live);
  if (!offline.length && !lost.length && !error) return null;
  const now = Date.now() / 1000;
  const line = (c: CopyRow, sub: string, buttons: { act: CopyAct; label: string; pri: boolean }[]): CopyLine => {
    const key = copyKey(c);
    return { key, title: c.path ? `${c.project} / ${c.path}` : c.project, sub: `${c.agent} · ${sub}`, buttons: buttons.map(b => ({ ...b, busy: busy.includes(key) })) };
  };
  return { error, rows: [
    ...offline.map(c => line(c, offlineLine(c, now), [{ act: "continue", label: "Continue on this computer", pri: true }])),
    ...lost.map(c => line(c, lostLine(), [{ act: "continue", label: "Restore", pri: false }, { act: "discard", label: discardLabel(armed === copyKey(c)), pri: false }])),
  ] };
}

/// A copy's button was clicked.
export function actOnCopy(key: string, act: CopyAct) {
  const { rows, armed, busy } = copies.getState(), c = rows.find(r => copyKey(r) === key);
  if (!c || busy.includes(key)) return;

  // Discarding a version is for good: the first click asks, in the row.
  if (act === "discard" && armed !== key) {
    copies.setState({ armed: key });
    return;
  }

  // One click is one request: a second would run the same copy again. The
  // row keeps how it looks until the copies are read again.
  copies.setState({ busy: [...busy, key] });
  invoke(act === "discard" ? "session_discard_copy" : "session_continue_copy", { agent: c.agent, session: c.id, lost: c.lost })
    .then(() => "", e => String(e))
    .then(async error => {
      void loadSessions().catch(() => []);
      copies.setState({ rows: await readCopies(), error, armed: "", busy: [] });
    });
}
