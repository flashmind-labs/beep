// Terminals in a coding agent session's pane: a shell on the session's machine
// in its folder, kept running by the node like any terminal (Home lists it
// under the machine).

import { endsForGood, fromBase64, newView, retryDelay, sizeChanged, splice, type Screen, type TermEvent, type TerminalRow, type TermSize } from "../terminals/terminals.ts";
import { Channel, invoke } from "../ui/tauri.ts";

/// The size a terminal starts at, before its screen is fitted.
const START_SIZE = { cols: 100, rows: 30 };
/// Attaches tried in a row before giving up until asked again.
const MAX_TRIES = 6;

export type PaneLink = "attaching" | "attached" | "detached" | "ended";

/// One terminal tab.
export interface PaneTerm {
  row: TerminalRow;
  screen: Screen;
  link: PaneLink;
  /// Bytes of output on the screen so far: where a reattach starts.
  next: number;
  tries: number;
  timer?: ReturnType<typeof setTimeout>;
  /// This tab's name for its stream: another view of the same terminal
  /// keeps its own.
  view: string;
  /// Counts attaches, so a stream that an attach replaced is ignored.
  attaches: number;
  /// The size the node last got from this tab.
  sent: TermSize | null;
  /// Its shell exited, or its machine no longer has it.
  onEnd: () => void;
  /// How it stands changed.
  onLink: () => void;
}

/// Where a new terminal starts: the session's machine and folder.
export interface TermPlace {
  machine_id: string;
  cwd: string;
  name: string;
}

/// Starts a shell at `place` and attaches to it.
export async function openTerm(place: TermPlace, hooks: Pick<PaneTerm, "onEnd" | "onLink">): Promise<PaneTerm> {
  const { makeScreen } = await import("../terminals/termview.ts");
  const row = await invoke<TerminalRow | null>("terminal_open", {
    terminal: { project: "", machineId: place.machine_id, name: place.name, command: "", cwd: place.cwd, ...START_SIZE },
  });
  if (!row) throw new Error("The server is too old for terminals here.");
  const t = paneTerm(row, makeScreen(), hooks);
  attach(t);
  return t;
}

/// Terminal `row` on `screen`, with typing and resizes wired to it; not
/// attached yet.
export function paneTerm(row: TerminalRow, screen: Screen, hooks: Pick<PaneTerm, "onEnd" | "onLink">): PaneTerm {
  const t: PaneTerm = { row, screen, link: "attaching", next: 0, tries: 0, view: newView(), attaches: 0, sent: null, ...hooks };
  screen.onData(data => {
    if (t.link === "attached") invoke("terminal_input", { id: row.id, view: t.view, data }).catch(() => {});
  });
  screen.onResize(() => { if (t.link === "attached") sendSize(t); });
  return t;
}

/// Tells the node the screen's size, when it changed since it last heard:
/// a resize makes the shell draw its prompt again.
function sendSize(t: PaneTerm) {
  const now = t.screen.size();
  if (!sizeChanged(t.sent, now)) return;
  t.sent = now;
  invoke("terminal_resize", { id: t.row.id, view: t.view, ...now }).catch(() => {});
}

/// Follows `t` from where its screen got to, until it ends or is detached;
/// a dropped stream is attached again after a wait, shown in the footer
/// rather than on the screen.
export function attach(t: PaneTerm) {
  clearTimeout(t.timer);
  t.link = "attaching";
  t.onLink();
  const attempt = ++t.attaches;
  // Whether this is still the tab's stream: one it replaced says nothing.
  const current = () => t.attaches === attempt;
  const channel = new Channel<TermEvent>();
  channel.onmessage = ev => {
    if (t.link === "detached" || !current()) return;
    if (ev.kind === "attached") {
      t.link = "attached";
      t.tries = 0;
      t.screen.setInput(true);
      t.screen.focus();
      // The screen may have been fitted while it attached.
      sendSize(t);
    } else if (ev.kind === "output") {
      const bytes = fromBase64(ev.data);
      const { skip, gap } = splice(t.next, ev.at, bytes.length);
      if (gap) t.screen.note("… earlier output was not kept");
      t.screen.write(bytes.subarray(skip));
      t.next = Math.max(t.next, ev.at + bytes.length);
      return;
    } else if (ev.kind === "ended") {
      t.link = "ended";
      t.screen.setInput(false);
      if (endsForGood(ev.state)) return t.onEnd();
      t.row = { ...t.row, state: ev.state, code: ev.code };
    } else {
      // A terminal that takes no input is not one the pane opened.
      t.link = "ended";
      t.screen.setInput(false);
    }
    t.onLink();
  };
  // The attach carries the size, so the node has it from here on.
  const size = t.screen.size();
  t.sent = size;
  invoke("terminal_attach", { id: t.row.id, view: t.view, from: t.next, ...size, onEvent: channel })
    .then(() => { if (current() && (t.link === "attached" || t.link === "attaching")) retry(t); })
    .catch(() => { if (current() && t.link !== "detached" && t.link !== "ended") retry(t); });
}

function retry(t: PaneTerm) {
  t.tries += 1;
  t.screen.setInput(false);
  t.link = t.tries > MAX_TRIES ? "detached" : "attaching";
  if (t.link === "attaching") t.timer = setTimeout(() => attach(t), retryDelay(t.tries));
  t.onLink();
}

/// Stops following `t` here; it keeps running on its machine.
export function detachTerm(t: PaneTerm) {
  clearTimeout(t.timer);
  const was = t.link;
  t.link = "detached";
  t.screen.setInput(false);
  if (was === "attached" || was === "attaching") invoke("terminal_detach", { id: t.row.id, view: t.view }).catch(() => {});
}

/// Detaches `t` and lets go of its screen.
export function dropTerm(t: PaneTerm) {
  detachTerm(t);
  t.screen.dispose();
}

/// Stops `t`'s shell on its machine.
export async function killTerm(t: PaneTerm) {
  await invoke("terminal_close", { id: t.row.id });
  dropTerm(t);
}

/// Forgets an ended terminal on the server, as Home does.
export function forgetTerm(t: PaneTerm) {
  dropTerm(t);
  invoke("terminal_close", { id: t.row.id }).catch(e => console.warn("closing an ended terminal:", e));
}

/// The footer line of a terminal tab.
export function termStatus(t: PaneTerm, machine: string) {
  switch (t.link) {
    case "attaching": return t.tries ? `Reconnecting to ${machine}…` : "Attaching…";
    case "attached": return `${machine} · what you type goes there`;
    case "detached": return "Detached: it keeps running";
    case "ended": return t.row.state === "offline" ? `${machine} is offline; it may still run there` : "Ended";
  }
}
