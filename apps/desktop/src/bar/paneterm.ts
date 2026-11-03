// Terminals in a coding agent session's pane: a shell on the session's
// machine in its folder, kept running by the node like any terminal (Home
// lists it under the machine). Each is a tab, attached live while the
// session view shows; its screen is xterm.js, loaded on first use. When
// its shell exits the tab goes.

import { endsForGood, fromBase64, retryDelay, splice, type Screen, type TermEvent, type TerminalRow } from "../terminals.ts";
import { Channel, invoke } from "./core.ts";

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
  const { makeScreen } = await import("../termview.ts");
  const row = await invoke<TerminalRow | null>("terminal_open", {
    terminal: { project: "", machineId: place.machine_id, name: place.name, command: "", cwd: place.cwd, ...START_SIZE },
  });
  if (!row) throw new Error("Beep's server needs an update before it opens a terminal here.");
  const screen = makeScreen();
  const t: PaneTerm = { row, screen, link: "attaching", next: 0, tries: 0, ...hooks };
  screen.onData(data => {
    if (t.link === "attached") invoke("terminal_input", { id: row.id, data }).catch(() => {});
  });
  screen.onResize(({ cols, rows }) => {
    if (t.link === "attached") invoke("terminal_resize", { id: row.id, cols, rows }).catch(() => {});
  });
  attach(t);
  return t;
}

/// Follows `t` from where its screen got to, until it ends or is detached;
/// a dropped stream is attached again after a wait.
export function attach(t: PaneTerm) {
  clearTimeout(t.timer);
  t.link = "attaching";
  t.onLink();
  const channel = new Channel<TermEvent>();
  channel.onmessage = ev => {
    if (t.link === "detached") return;
    if (ev.kind === "attached") {
      t.link = "attached";
      t.tries = 0;
      t.screen.setInput(true);
      t.screen.focus();
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
  const { cols, rows } = t.screen.size();
  invoke("terminal_attach", { id: t.row.id, from: t.next, cols, rows, onEvent: channel })
    .then(() => { if (t.link === "attached" || t.link === "attaching") retry(t); })
    .catch(() => { if (t.link !== "detached" && t.link !== "ended") retry(t); });
}

function retry(t: PaneTerm) {
  t.tries += 1;
  t.screen.setInput(false);
  if (t.tries > MAX_TRIES) {
    t.link = "detached";
    t.onLink();
    return;
  }
  t.link = "attaching";
  t.screen.note("Reconnecting…");
  t.timer = setTimeout(() => attach(t), retryDelay(t.tries));
  t.onLink();
}

/// Stops following `t` here; it keeps running on its machine.
export function detachTerm(t: PaneTerm) {
  clearTimeout(t.timer);
  const was = t.link;
  t.link = "detached";
  t.screen.setInput(false);
  if (was === "attached" || was === "attaching") invoke("terminal_detach", { id: t.row.id }).catch(() => {});
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
    case "attaching": return "Attaching…";
    case "attached": return `${machine} · what you type goes there`;
    case "detached": return "Detached: it keeps running";
    case "ended": return t.row.state === "offline" ? `${machine} is offline; it may still run there` : "Ended";
  }
}
