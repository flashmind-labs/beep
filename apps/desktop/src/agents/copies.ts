// The copies this Mac keeps of other machines' sessions, in the Coding
// agents list: a session whose machine is off offers Continue on this Mac,
// and a version that lost to another machine's can be restored or thrown
// away. The node brings the copies in on its own; nothing here starts one.

import { type CopyRow, copyKey, copyLists, lostLine, offlineLine } from "./copy-state.ts";
import { esc } from "../ui/format.ts";
import { loadSessions } from "./sessions.ts";
import { invoke } from "../ui/tauri.ts";

const CP = { rows: [] as CopyRow[], error: "" };
const listeners = new Set<() => void>();

/// Calls `f` whenever the copies change, for the list to draw again.
export const onCopies = (f: () => void) => void listeners.add(f);
const changed = () => { for (const f of listeners) f(); };

/// Reads the copies the node keeps.
export async function loadCopies() {
  try { CP.rows = await invoke<CopyRow[]>("session_copies"); }
  catch (e) { console.warn("copies not read", e); CP.rows = []; }
  changed();
}

/// The rows to draw under the sessions, as HTML; `live` are the ids of the
/// sessions the machines list, `online` the machines that are on.
export function copiesHtml(live: ReadonlySet<string>, online: ReadonlySet<string>, machine: (id: string) => string) {
  const { offline, lost } = copyLists(CP.rows, live, online);
  if (!offline.length && !lost.length && !CP.error) return "";
  const now = Date.now() / 1000;
  const row = (c: CopyRow, line: string, actions: string) =>
    `<div class="dchat copy" data-cpk="${esc(copyKey(c))}"><span class="t"><span>${esc(c.project)}${c.path ? ` / ${esc(c.path)}` : ""}</span><small>${esc(c.agent)} · ${esc(line)}</small></span>${actions}</div>`;
  const go = (label: string, act: string, pri: boolean) => `<button class="dbtn ${pri ? "pri " : "quiet "}small" data-cpact="${act}">${label}</button>`;
  return `<div class="dlist dcopies">${CP.error ? `<div class="derr dpadx">${esc(CP.error)}</div>` : ""}`
    + offline.map(c => row(c, offlineLine(machine(c.from), c, now), go("Continue on this Mac", "continue", true))).join("")
    + lost.map(c => row(c, lostLine(), go("Restore", "continue", false) + go("Discard", "discard", false))).join("")
    + `</div>`;
}

/// Handles a click on one of the rows; true when it was one.
export function copiesClick(target: HTMLElement): boolean {
  const button = target.closest<HTMLElement>("[data-cpact]");
  const key = button?.closest<HTMLElement>("[data-cpk]")?.dataset.cpk;
  const c = CP.rows.find(r => copyKey(r) === key);
  if (!button || !c) return false;
  const command = button.dataset.cpact === "discard" ? "session_discard_copy" : "session_continue_copy";
  CP.error = "";
  invoke(command, { agent: c.agent, session: c.id, lost: c.lost })
    .catch(e => { CP.error = String(e); })
    .then(() => Promise.all([loadCopies(), loadSessions().catch(() => [])]));
  return true;
}
