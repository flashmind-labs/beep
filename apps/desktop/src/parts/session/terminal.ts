// Terminals in a coding agent session's pane: a shell on the session's machine
// in its folder, kept running by the node like any terminal (Home lists it
// under the machine).

import { changeTerminalLink, describeMessage, endsForGood, inOrderChannel, newView, readTermMessage, Replay, sizeChanged, splice, terminalAttempt, terminalFollowing, type Screen, type TerminalLink, type TerminalRow, type TermSize } from "../../stores/terminals/terminal-state.ts";
import { indexedChannel, invoke, logError, logInfo } from "../../ui/tauri.ts";
import { onShownChange } from "../../ui/active.ts";
import { RETRY_MS } from "../../stores/agents/reconnect-state.ts";

/// The size a terminal starts at, before its screen is fitted.
const START_SIZE = { cols: 100, rows: 30 };

/// One terminal tab.
export interface PaneTerm {
  row: TerminalRow;
  screen: Screen;
  link: TerminalLink;
  /// Bytes of output on the screen so far: where a reattach starts.
  next: number;
  /// This tab's name for its stream: another view of the same terminal
  /// keeps its own.
  view: string;
  /// The size the node last got from this tab.
  sent: TermSize | null;
  /// Which output replays what the node kept.
  replay: Replay;
  /// Its shell exited, or its machine no longer has it.
  onEnd: () => void;
  /// How it stands changed.
  onLink: () => void;
  /// Esc was typed in it: the conversation's input takes the keys.
  onEscape: () => void;
}

/// Where a new terminal starts: a machine and folder, and the project it
/// belongs to ("" for none), which lists it on the project's page.
export interface TermPlace {
  machine_id: string;
  machine_name: string;
  cwd: string;
  name: string;
  project: string;
}

type TermHooks = Pick<PaneTerm, "onEnd" | "onLink" | "onEscape">;

/// Starts a shell at `place` and attaches to it.
export async function openTerm(place: TermPlace, hooks: TermHooks): Promise<PaneTerm> {
  const row = await invoke<TerminalRow>("terminal_open", {
    terminal: { project: place.project, machineId: place.machine_id, name: place.name, command: "", cwd: place.cwd, ...START_SIZE },
  });
  return followTerm(row, hooks, true);
}

/// Follows terminal `row`, one that already runs, in a tab; `opened` when
/// this tab just started it.
export async function followTerm(row: TerminalRow, hooks: TermHooks, opened = false): Promise<PaneTerm> {
  const { makeScreen } = await import("../terminals/termview.ts");
  const t = paneTerm(row, makeScreen(), hooks, opened);
  attach(t);
  return t;
}

/// Terminal `row` on `screen`, with typing and resizes wired to it; not
/// attached yet.
export function paneTerm(row: TerminalRow, screen: Screen, hooks: TermHooks, opened = false): PaneTerm {
  const t: PaneTerm = { row, screen, link: { kind: "detached" }, next: 0, view: newView(), sent: null, replay: new Replay(opened), ...hooks };
  const input = (data: string) => {
    if (t.link.kind === "attached") invoke("terminal_input", { id: row.id, view: t.view, data }).catch(() => {});
  };
  screen.onData(input);
  // The node does not answer queries yet: the screen's answers go as input.
  screen.onReply(input);
  screen.onResize(() => { if (t.link.kind === "attached") sendSize(t); });
  screen.onEscape(() => t.onEscape());
  follow(t);
  return t;
}

/// Tabs that may be following their terminal. While the pane is off screen
/// they let go, and when it shows they attach again from `next`, so the
/// output from meanwhile arrives in order and the screen keeps its state.
const tabs = new Set<PaneTerm>();
let watching = false;

function follow(t: PaneTerm) {
  tabs.add(t);
  if (watching || typeof document === "undefined") return;
  watching = true;
  onShownChange(shown => { for (const tab of tabs) if (shown) resume(tab); else pause(tab); }, true);
}

function pause(t: PaneTerm) {
  if (terminalFollowing(t.link)) leave(t, { kind: "paused" });
}

function resume(t: PaneTerm) {
  if (t.link.kind === "paused") attach(t);
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
export function attach(t: PaneTerm, tries = 0) {
  if (t.link.kind === "disposed" || t.link.kind === "closing") return;
  const attempt = { cancel: () => {} };
  changeTerminalLink(t, { kind: "attaching", attempt, tries });
  const current = () => terminalAttempt(t.link) === attempt;
  t.onLink();
  if (!current()) return;

  const show = (message: unknown) => {
    const ev = readTermMessage(message);
    if (ev === null) {
      logError(`terminal ${t.row.id}: a stream message was not understood (${describeMessage(message)})`);
      return;
    }

    if (ev.kind === "attached") {
      logInfo(`terminal ${t.row.id}: attached in a pane`);
      t.replay.start(performance.now());
      changeTerminalLink(t, { kind: "attached", attempt });
      t.screen.setInput(true);
      // The screen may have been fitted while it attached.
      sendSize(t);
    } else if (ev.kind === "output") {
      const bytes = ev.data;
      const { skip, gap } = splice(t.next, ev.at, bytes.length);
      if (gap) t.screen.note("… earlier output was not kept");
      if (t.replay.is(performance.now())) t.screen.replay(bytes.subarray(skip));
      else t.screen.write(bytes.subarray(skip));
      t.next = Math.max(t.next, ev.at + bytes.length);
      return;
    } else if (ev.kind === "ended") {
      logInfo(`terminal ${t.row.id}: ended (${ev.state})`);
      changeTerminalLink(t, { kind: "ended" });
      t.screen.setInput(false);
      if (endsForGood(ev.state)) return t.onEnd();
      t.row = { ...t.row, state: ev.state, code: ev.code };
    } else {
      // A terminal that takes no input is not one the pane opened.
      logInfo(`terminal ${t.row.id}: view only in a pane`);
      changeTerminalLink(t, { kind: "ended" });
      t.screen.setInput(false);
    }

    t.onLink();
  };
  const stream = inOrderChannel(indexedChannel, current, show, missing => {
    logError(`terminal ${t.row.id}: stream message ${missing} was lost in a pane; attaching again`);
    attach(t, t.link.kind === "attaching" ? t.link.tries : 0);
  });
  attempt.cancel = stream.cancel;
  // The attach carries the size, so the node has it from here on.
  const size = t.screen.size();
  t.sent = size;
  invoke("terminal_attach", { id: t.row.id, view: t.view, from: t.next, ...size, onEvent: stream.channel })
    .then(() => { if (current()) retry(t); })
    .catch(() => { if (current()) retry(t); });
}

function retry(t: PaneTerm) {
  const tries = (t.link.kind === "attaching" ? t.link.tries : 0) + 1;
  logInfo(`terminal ${t.row.id}: stream dropped in a pane, try ${tries}`);
  t.screen.setInput(false);
  changeTerminalLink(t, { kind: "retrying", tries, timer: setTimeout(() => attach(t, tries), RETRY_MS) });
  t.onLink();
}

/// Stops following `t` here; it keeps running on its machine.
export function detachTerm(t: PaneTerm) {
  if (t.link.kind !== "disposed" && t.link.kind !== "closing") leave(t, { kind: "detached" });
}

function leave(t: PaneTerm, link: TerminalLink) {
  const following = terminalFollowing(t.link);
  changeTerminalLink(t, link);
  t.screen.setInput(false);
  if (following) invoke("terminal_detach", { id: t.row.id, view: t.view }).catch(() => {});
}

/// Detaches `t` and lets go of its screen.
export function dropTerm(t: PaneTerm) {
  if (t.link.kind === "disposed") return;
  tabs.delete(t);
  leave(t, { kind: "disposed" });
  t.screen.dispose();
}

/// Stops `t`'s shell on its machine and lets go of its screen. The tab
/// stops following it first, so the end the stop causes is not taken for
/// one of its own; when the stop fails it follows it again.
export function killTerm(t: PaneTerm): Promise<void> {
  const was = t.link;
  if (was.kind === "closing") return was.completion;

  const completion = Promise.resolve().then(stop);
  if (was.kind !== "disposed") {
    leave(t, { kind: "closing", completion });
    t.onLink();
  }
  return completion;

  async function stop() {
    try {
      await invoke("terminal_close", { id: t.row.id });
    } catch (e) {
      if (t.link.kind === "closing") {
        if (terminalFollowing(was)) {
          changeTerminalLink(t, { kind: "detached" });
          attach(t);
        } else {
          changeTerminalLink(t, was);
          t.onLink();
        }
      }

      throw e;
    }

    dropTerm(t);
  }
}

/// Forgets an ended terminal on its machine, as Home does.
export function forgetTerm(t: PaneTerm) {
  dropTerm(t);
  invoke("terminal_close", { id: t.row.id }).catch(e => console.warn("closing an ended terminal:", e));
}

/// The footer line of a terminal tab.
export function termStatus(t: PaneTerm, machine: string) {
  switch (t.link.kind) {
    case "attaching": return t.link.tries ? `Reconnecting to ${machine}…` : "Attaching…";
    case "retrying": return `Reconnecting to ${machine}…`;
    case "attached": return `${machine} · what you type goes there`;
    case "closing": return "Closing…";
    case "paused":
    case "disposed":
    case "detached": return "Detached: it keeps running";
    case "view_only":
    case "ended": return t.row.state === "offline" ? `${machine} is offline; it may still run there` : "Ended";
  }
}
