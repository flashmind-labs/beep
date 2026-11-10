// The copies this Mac keeps of other machines' sessions, in the Coding
// agents list: a session whose machine is off offers Continue on this Mac,
// and a version that lost to another machine's can be restored or thrown
// away. The node brings the copies in on its own; nothing here starts one.
import { createStore } from "zustand/vanilla";
import { subscribeChanges, topic } from "../ui/mesh-events.ts";
import { type CopyRow, copyKey, copyLists, discardLabel, lostLine, offlineLine } from "./copy-state.ts";
import { loadSessions } from "./sessions.ts";
import { esc } from "../ui/format.ts";
import { invoke } from "../ui/tauri.ts";
import { pageShown } from "../ui/active.ts";

/// The copies read, the last failure, and the copy whose discard asks to be confirmed.
export const copies = createStore(() => ({ rows: [] as CopyRow[], error: "", armed: "" }));

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

/// The rows to draw under the sessions, as HTML; `live` are the ids of the
/// sessions the machines list.
export function copiesHtml(live: ReadonlySet<string>) {
  const { rows, error, armed } = copies.getState();
  const { offline, lost } = copyLists(rows, live);
  if (!offline.length && !lost.length && !error) return "";
  const now = Date.now() / 1000;
  const row = (c: CopyRow, line: string, actions: string) =>
    `<div class="dchat copy" data-cpk="${esc(copyKey(c))}"><span class="t"><span>${esc(c.project)}${c.path ? ` / ${esc(c.path)}` : ""}</span><small>${esc(c.agent)} · ${esc(line)}</small></span>${actions}</div>`;
  const go = (label: string, act: string, pri: boolean) => `<button class="dbtn ${pri ? "pri " : "quiet "}small" data-cpact="${act}">${label}</button>`;
  return `<div class="dlist dcopies">${error ? `<div class="derr dpadx">${esc(error)}</div>` : ""}`
    + offline.map(c => row(c, offlineLine(c, now), go("Continue on this computer", "continue", true))).join("")
    + lost.map(c => row(c, lostLine(), go("Restore", "continue", false) + go(discardLabel(armed === copyKey(c)), "discard", false))).join("")
    + `</div>`;
}

/// Handles a click on one of the rows; true when it was one.
export function copiesClick(target: HTMLElement): boolean {
  const button = target.closest<HTMLElement>("[data-cpact]");
  const key = button?.closest<HTMLElement>("[data-cpk]")?.dataset.cpk;
  const c = copies.getState().rows.find(r => copyKey(r) === key);
  if (!button || !c) return false;
  const command = button.dataset.cpact === "discard" ? "session_discard_copy" : "session_continue_copy";
  if (button.hasAttribute("disabled")) return true;

  // Discarding a version is for good: the first click asks, in the row.
  if (button.dataset.cpact === "discard" && copies.getState().armed !== key) {
    copies.setState({ armed: key ?? "" });
    return true;
  }

  // One click is one request: a second would run the same copy again. The
  // row keeps how it looks until the copies are read again.
  button.setAttribute("disabled", "");
  invoke(command, { agent: c.agent, session: c.id, lost: c.lost })
    .then(() => "", e => String(e))
    .then(async error => {
      void loadSessions().catch(() => []);
      copies.setState({ rows: await readCopies(), error, armed: "" });
    });
  return true;
}
