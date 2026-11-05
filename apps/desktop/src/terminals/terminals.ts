// terminals/: terminals kept running on the machines, and the window's
// Terminals page (view.ts). Entry: makeTerminals here.
// Terminals: commands and shells kept running on the user's machines, apart
// from any chat, like tmux sessions: the node owns them, so they keep running
// with the window closed and can be attached to again.
import type { invoke as Invoke, Channel as ChannelCtor } from "../ui/tauri.ts";
import { emptyPanel } from "../ui/parts.ts";
import { press } from "../ui/dom.ts";
import { confirmSheet, openMenu, type MenuItem } from "../ui/menu.ts";
import type { Ago, MakeButton, MakeEl } from "../ui/dom.ts";
import type { TileData } from "../project/board.ts";
import { MACHINE_REFRESH_MS, PageCache, type SectionStore } from "../ui/pagecache.ts";
import { reason } from "../ui/format.ts";

/// A terminal (TerminalRow).
export interface TerminalRow {
  id: string;
  /// `org/repo`, or empty for a terminal of no project.
  project: string;
  machine_id: string;
  machine_name: string;
  name: string;
  command: string;
  cwd: string;
  /// `running`, `exited`, `stopped`, `offline` or `gone`.
  state: string;
  /// The exit code once it exited, when it gave one.
  code: number | null;
  /// In milliseconds since the Unix epoch.
  started_at: number;
}

/// A terminal with its last lines, as `terminal_output` gives it (OutputView).
export interface OutputView {
  terminal: TerminalRow;
  lines: string[];
  /// Lines came before these.
  earlier: boolean;
}

/// What an attached terminal sends (TermEvent): output from byte `at` in
/// base64, its end, or that it takes no input.
export type TermEvent =
  | { kind: "attached" }
  | { kind: "output"; at: number; data: string }
  | { kind: "ended"; state: string; code: number | null }
  | { kind: "view_only" };

/// A machine a terminal can start on, in the project's folder there.
export interface TerminalPlace {
  id: string;
  name: string;
  folder: string;
}

/// A terminal emulator on the page (termview.ts draws one with xterm.js).
export interface Screen {
  el: HTMLElement;
  write(data: Uint8Array): void;
  /// A dim line of our own, such as "… earlier output was not kept".
  note(text: string): void;
  size(): { cols: number; rows: number };
  /// Fits the screen to its element, once it is on the page.
  fit(): void;
  focus(): void;
  /// Whether the keyboard is in it now.
  focused(): boolean;
  onData(cb: (data: string) => void): void;
  onResize(cb: (size: { cols: number; rows: number }) => void): void;
  /// Whether keys typed go anywhere.
  setInput(on: boolean): void;
  dispose(): void;
}

/// A machine as the Terminals page lists it (MachineView).
export interface TerminalMachine {
  id: string;
  name: string;
  online: boolean;
}

/// One machine's terminals on the Terminals page and Home.
export interface MachineTerminals<T> {
  id: string;
  name: string;
  online: boolean;
  terms: T[];
}

/// The scope of the Terminals page: every terminal, of any project or none.
export const ALL = "";

/// What the window hands `makeTerminals`.
export interface TerminalDeps {
  invoke: typeof Invoke;
  Channel: typeof ChannelCtor;
  makeScreen: () => Screen;
  h: MakeEl;
  button: MakeButton;
  ago: Ago;
  redraw: () => void;
  /// Where each project's list is kept between windows; in memory only
  /// without it.
  kept?: SectionStore<TerminalRow[] | null>;
}

/// The New terminal form as typed so far.
interface TerminalForm {
  command: string;
  name: string;
  machine: string | null;
  cwd: string;
}

/// How an open terminal's live view stands.
type Link = "attaching" | "attached" | "detached" | "ended" | "view_only";

/// The open terminal's live view: its screen and how far its output got.
interface Live {
  id: string;
  screen: Screen;
  link: Link;
  /// Bytes of output on the screen so far: where a reattach starts.
  next: number;
  /// Typing is off by choice ("View only").
  watching: boolean;
  /// Failed attaches in a row, for the wait before the next.
  tries: number;
  timer?: ReturnType<typeof setTimeout>;
  /// This view's name for its stream.
  view: string;
  /// Counts attaches, so a stream that an attach replaced is ignored.
  attaches: number;
  /// The size the node last got from this view.
  sent: TermSize | null;
}

/// One project's terminals: `list` is undefined while it loads and null
/// from a server too old to keep terminals.
interface ProjectTerminals {
  list: TerminalRow[] | null | undefined;
  open: string | null;
  output: OutputView | null;
  form: TerminalForm | null;
  error: string;
  busy: boolean;
}

/// A machine row as `terminalPlaces` reads it: `whereRows` gives them.
type Where = { kind: string; id: string; name: string; folder: string | null };

/// How often a view-only terminal's output is read again.
const POLL_MS = 2000;
/// The size a terminal starts at, before its screen is fitted.
const START_SIZE = { cols: 100, rows: 30 };
/// Attaches tried in a row before giving up until asked again.
const MAX_TRIES = 6;

/// Every machine with its terminals, in the machines' order; a terminal of
/// a machine no longer listed goes under its own name at the end, offline.
export function byMachine<T extends Pick<TerminalRow, "machine_id" | "machine_name">>(machines: readonly TerminalMachine[], list: readonly T[]): MachineTerminals<T>[] {
  const out: MachineTerminals<T>[] = machines.map(m => ({ id: m.id, name: m.name, online: m.online, terms: [] }));
  for (const t of list) {
    let m = out.find(x => x.id === t.machine_id);
    if (!m) out.push(m = { id: t.machine_id, name: t.machine_name || "Removed machine", online: false, terms: [] });
    m.terms.push(t);
  }
  return out;
}

/// Where a terminal runs: its project and folder, or the folder alone.
export function terminalWhere(t: Pick<TerminalRow, "project" | "cwd">) {
  return t.project ? `${t.project} · ${t.cwd}` : t.cwd;
}

/// How a terminal stands in a list: how long it has run, or how it ended,
/// with its exit code.
export function terminalNote(t: Pick<TerminalRow, "state" | "started_at"> & Partial<Pick<TerminalRow, "code">>, ago: Ago) {
  if (t.state === "running") return ago(t.started_at);
  if (t.state === "exited" && t.code != null) return `exited ${t.code}`;
  return stateLabel(t);
}

/// "N on M machines", counting the machines that have any.
export function terminalsCount(groups: readonly MachineTerminals<unknown>[]) {
  const n = groups.reduce((a, m) => a + m.terms.length, 0);
  const on = groups.filter(m => m.terms.length).length;
  if (!n) return "None open";
  return `${n} on ${on} ${on === 1 ? "machine" : "machines"}`;
}

/// "running", "exited 1", "stopped", "offline", "gone".
export function stateLabel(t: Pick<TerminalRow, "state"> & Partial<Pick<TerminalRow, "code">>) {
  if (t.state === "exited") return t.code == null ? "ended" : t.code === 0 ? "done" : `exited ${t.code}`;
  return t.state;
}

/// The board's Terminals tile from the project's terminals; undefined while
/// they load, null from a server too old to keep them.
export function terminalTile(
  list: readonly (Pick<TerminalRow, "name" | "state" | "started_at"> & Partial<Pick<TerminalRow, "code" | "machine_name" | "command" | "cwd">>)[] | null | undefined,
  ago: Ago,
): TileData {
  if (list === undefined) return { count: "", rows: [], empty: "Loading…" };
  if (list === null) return { count: "", rows: [], empty: "The server keeps no terminals" };
  const running = list.filter(t => t.state === "running").length;
  return {
    count: running ? `${running} running` : list.length ? String(list.length) : "",
    rows: list.map(t => ({
      code: t.name,
      dot: t.state === "running" ? "" : "off",
      note: t.state === "running" ? ago(t.started_at) : stateLabel(t),
      title: `${t.machine_name}: ${t.command} in ${t.cwd}`,
    })),
    empty: "No terminals yet",
  };
}

/// The machines a terminal can start on: those with a checkout, each with
/// its folder.
export function terminalPlaces(where: readonly Where[] | null | undefined): TerminalPlace[] {
  return (where || []).filter((r): r is Where & { folder: string } => r.kind === "here" && !!r.folder).map(r => ({ id: r.id, name: r.name, folder: r.folder }));
}

/// Where a chunk of `len` bytes from byte `at` meets the `next` byte the
/// screen waits for: `skip` leading bytes it already shows, and `gap` when
/// bytes before it were no longer kept.
export function splice(next: number, at: number, len: number): { skip: number; gap: boolean } {
  if (at > next) return { skip: 0, gap: true };
  return { skip: Math.min(next - at, len), gap: false };
}

/// The bytes of a base64 string.
export function fromBase64(data: string): Uint8Array {
  const text = atob(data);
  const out = new Uint8Array(text.length);
  for (let i = 0; i < text.length; i++) out[i] = text.charCodeAt(i);
  return out;
}

/// A terminal's size in cells.
export interface TermSize {
  cols: number;
  rows: number;
}

/// Whether `now` differs from `sent`, the size the node last got. A resize
/// to the size it already has makes the shell draw its prompt again.
export function sizeChanged(sent: TermSize | null, now: TermSize) {
  return !sent || sent.cols !== now.cols || sent.rows !== now.rows;
}

/// A name for one view of a terminal: two views of the same terminal
/// each keep their own stream.
export function newView(): string {
  return crypto.randomUUID();
}

/// How long to wait before attach number `tries` (1 for the first retry).
export function retryDelay(tries: number) {
  return Math.min(1000 * 2 ** Math.max(0, tries - 1), 15000);
}

/// Whether a terminal that ended in `state` is done for good: it exited or was
/// stopped, or its machine no longer has it.
export function endsForGood(state: string) {
  return state === "exited" || state === "stopped" || state === "gone";
}

/// The footer line of an open terminal.
export function linkLabel(link: Link, watching: boolean, machine: string, tries = 0) {
  switch (link) {
    case "attaching": return tries ? `Reconnecting to ${machine}…` : "Attaching…";
    case "attached": return watching ? "Watching: typing is off" : `Attached: what you type goes to ${machine}`;
    case "detached": return "Detached: it keeps running";
    case "ended": return "Ended";
    case "view_only": return "View only: the last 100 lines, read every 2 seconds";
  }
}

export function makeTerminals(deps: TerminalDeps) {
  const { invoke, Channel, makeScreen, h, button, ago, redraw } = deps;
  const all = new Map<string, ProjectTerminals>();
  /// Live views by terminal id, for the project on show.
  const lives = new Map<string, Live>();
  let timer: ReturnType<typeof setInterval> | undefined;
  let polling: string | null = null;
  /// The project whose terminals view shows.
  let shown: string | null = null;

  const kept = deps.kept ?? new PageCache(null).section<TerminalRow[] | null>("terminals");
  /// The lists being read: a load meanwhile is dropped.
  const reading = new Set<string>();

  /// A project's terminals, from the cache until they are read.
  const of = (name: string) => {
    let t = all.get(name);
    if (!t) all.set(name, t = { list: name === ALL ? undefined : kept.get(name), open: null, output: null, form: null, error: "", busy: false });
    return t;
  };

  async function load(name: string) {
    if (reading.has(name)) return;
    reading.add(name);
    const t = of(name);
    try {
      t.list = name === ALL
        ? await invoke<TerminalRow[] | null>("terminals_all")
        : await invoke<TerminalRow[] | null>("terminals_list", { project: name });
      if (t.open && !t.list?.some(x => x.id === t.open)) t.open = null;
      if (name !== ALL) kept.put(name, t.list);
    } catch (e) {
      t.list ??= [];
      t.error = reason(e);
    } finally {
      reading.delete(name);
    }
    redraw();
  }

  /// Reads project `name`'s list when the last read is `every` old or
  /// there was none. The read, when one starts.
  function ensure(name: string, every = MACHINE_REFRESH_MS): Promise<void> | undefined {
    if (kept.due(name, every) && !reading.has(name)) return load(name);
  }

  async function readOutput(name: string) {
    const t = of(name);
    if (!t.open) return;
    const id = t.open;
    try {
      const out = await invoke<OutputView>("terminal_output", { id });
      if (t.open !== id) return;
      t.output = out;
      setRow(name, out.terminal);
      t.error = "";
    } catch (e) {
      t.error = reason(e);
    }
    redraw();
  }

  function setRow(name: string, row: TerminalRow) {
    const t = of(name);
    t.list = (t.list || []).map(x => (x.id === row.id ? row : x));
  }

  /// Polls the open view-only terminal of `name`; `null` stops.
  function watch(name: string | null) {
    if (polling === name) return;
    clearInterval(timer);
    polling = name;
    if (!name) return;
    timer = setInterval(() => {
      if (document.visibilityState === "visible") readOutput(name);
    }, POLL_MS);
  }

  /// The live view of terminal `id`, made and attached the first time.
  function liveOf(name: string, id: string) {
    let live = lives.get(id);
    if (!live) {
      const screen = makeScreen();
      live = { id, screen, link: "attaching", next: 0, watching: false, tries: 0, view: newView(), attaches: 0, sent: null };
      lives.set(id, live);
      const l = live;
      screen.onData(data => {
        if (l.link !== "attached" || l.watching) return;
        invoke("terminal_input", { id, view: l.view, data }).catch(e => { of(name).error = reason(e); redraw(); });
      });
      screen.onResize(() => { if (l.link === "attached") sendSize(l); });
      attach(name, live);
    }
    return live;
  }

  /// Tells the node the screen's size, when it changed since it last heard.
  function sendSize(live: Live) {
    const now = live.screen.size();
    if (!sizeChanged(live.sent, now)) return;
    live.sent = now;
    invoke("terminal_resize", { id: live.id, view: live.view, ...now }).catch(() => {});
  }

  /// Follows `live` from where its screen got to, until it ends or is
  /// detached; a dropped stream is attached again after a wait, shown in
  /// the footer rather than on the screen.
  function attach(name: string, live: Live) {
    clearTimeout(live.timer);
    live.link = "attaching";
    const attempt = ++live.attaches;
    // Whether this is still the view's stream: one it replaced says nothing.
    const current = () => lives.get(live.id) === live && live.attaches === attempt;
    const channel = new Channel<TermEvent>();
    channel.onmessage = ev => {
      if (!current()) return;
      if (ev.kind === "attached") {
        live.link = "attached";
        live.tries = 0;
        live.screen.setInput(!live.watching);
        // The screen may have been fitted while it attached.
        sendSize(live);
        redraw();
      } else if (ev.kind === "output") {
        const bytes = fromBase64(ev.data);
        const { skip, gap } = splice(live.next, ev.at, bytes.length);
        if (gap) live.screen.note("… earlier output was not kept");
        live.screen.write(bytes.subarray(skip));
        live.next = Math.max(live.next, ev.at + bytes.length);
      } else if (ev.kind === "ended") {
        live.link = "ended";
        live.screen.setInput(false);
        // Its shell exited: the terminal goes from the list and its view
        // closes. One whose machine went away may still run there.
        if (endsForGood(ev.state)) return forget(name, live.id);
        const row = (of(name).list || []).find(x => x.id === live.id);
        if (row) setRow(name, { ...row, state: ev.state, code: ev.code });
        redraw();
      } else {
        live.link = "view_only";
        live.screen.setInput(false);
        readOutput(name);
        redraw();
      }
    };
    // The attach carries the size, so the node has it from here on.
    const size = live.screen.size();
    live.sent = size;
    invoke("terminal_attach", { id: live.id, view: live.view, from: live.next, ...size, onEvent: channel })
      .then(() => {
        // Ended, view only or detached on purpose; else the stream dropped.
        if (current() && (live.link === "attached" || live.link === "attaching")) retry(name, live, "");
      })
      .catch(e => {
        if (!current() || live.link === "detached") return;
        const why = reason(e);
        if (/needs an update/i.test(why)) {
          live.link = "view_only";
          readOutput(name);
          redraw();
          return;
        }
        retry(name, live, why);
      });
  }

  function retry(name: string, live: Live, why: string) {
    live.tries += 1;
    live.screen.setInput(false);
    if (live.tries > MAX_TRIES) {
      live.link = "detached";
      of(name).error = why || "The terminal's stream keeps dropping.";
      redraw();
      return;
    }
    live.link = "attaching";
    live.timer = setTimeout(() => attach(name, live), retryDelay(live.tries));
    redraw();
  }

  function detach(live: Live) {
    clearTimeout(live.timer);
    live.link = "detached";
    live.screen.setInput(false);
    invoke("terminal_detach", { id: live.id, view: live.view }).catch(() => {});
  }

  /// Detaches and drops every live view, such as when the view closes.
  function dropAll() {
    for (const live of lives.values()) {
      if (live.link === "attached" || live.link === "attaching") detach(live);
      live.screen.dispose();
    }
    lives.clear();
  }

  function select(name: string, id: string) {
    const t = of(name);
    t.open = id;
    t.form = null;
    t.output = null;
    if ((t.list || []).some(x => x.id === id)) {
      const live = liveOf(name, id);
      queueMicrotask(() => live.screen.focus());
    }
    redraw();
  }

  async function start(name: string, form: TerminalForm) {
    const t = of(name);
    t.busy = true;
    t.error = "";
    redraw();
    const fields = { project: name, machineId: form.machine, name: form.name, command: form.command, cwd: form.cwd };
    try {
      let made = await invoke<TerminalRow | null>("terminal_open", { terminal: { ...fields, ...START_SIZE } });
      // A server from before interactive terminals runs commands view only.
      if (!made) {
        if (!form.command.trim()) throw new Error("The server needs an update before it runs a shell.");
        made = await invoke<TerminalRow>("terminal_start", { terminal: fields });
      }
      t.list = [...(t.list || []), made];
      t.form = null;
      select(name, made.id);
    } catch (e) {
      t.error = reason(e);
    } finally {
      t.busy = false;
    }
    redraw();
  }

  /// Drops ended terminal `id` here and on the server.
  function forget(name: string, id: string) {
    const live = lives.get(id);
    if (live) {
      clearTimeout(live.timer);
      live.screen.dispose();
      lives.delete(id);
    }
    const t = of(name);
    t.list = (t.list || []).filter(x => x.id !== id);
    if (t.open === id) { t.open = null; t.output = null; }
    // Its row on the server; the next listing drops it too when this fails.
    invoke("terminal_close", { id }).catch(e => console.warn("closing an ended terminal:", e));
    redraw();
  }

  async function close(name: string, id: string) {
    const t = of(name);
    t.busy = true;
    redraw();
    try {
      await invoke("terminal_close", { id });
      const live = lives.get(id);
      if (live) {
        detach(live);
        live.screen.dispose();
        lives.delete(id);
      }
      t.list = (t.list || []).filter(x => x.id !== id);
      if (t.open === id) { t.open = null; t.output = null; }
      t.error = "";
    } catch (e) {
      t.error = reason(e);
    } finally {
      t.busy = false;
    }
    redraw();
  }

  /// The New terminal form. On the Terminals page (`ALL`) the folder may
  /// stay empty, for the machine's workspace or home folder.
  function formView(name: string, places: TerminalPlace[]) {
    const t = of(name);
    const anywhere = name === ALL;
    const f = t.form!;
    // A redraw while typing keeps the field and the caret.
    const was = document.activeElement?.closest?.(".tm-form") ? document.activeElement : null;
    const label = was?.getAttribute("aria-label"), caret = was instanceof HTMLInputElement ? was.selectionStart : undefined;
    if (label) queueMicrotask(() => {
      const again = document.querySelector<HTMLInputElement>(`.tm-form input[aria-label="${label}"]`);
      again?.focus();
      if (again && caret != null) again.setSelectionRange(caret, caret);
    });
    const input = (key: "command" | "name" | "cwd", label: string, ph: string) => {
      const field = h("input", "");
      field.value = f[key];
      field.placeholder = ph;
      field.spellcheck = false;
      field.setAttribute("aria-label", label);
      field.addEventListener("input", () => { f[key] = field.value; });
      return h("label", "tm-f", h("span", "", label), field);
    };
    const chips = h("div", "tm-chips", ...places.map(m => {
      return press(button(m.name, "dchip", () => { f.machine = m.id; if (!anywhere) f.cwd = m.folder; redraw(); }), f.machine === m.id);
    }));
    const go = button(t.busy ? "Starting…" : anywhere ? "Open" : "Start", "pri", () => {
      if (f.machine && (anywhere || f.cwd.trim())) start(name, f);
    });
    go.disabled = t.busy || !places.length;
    const machine = h("div", "tm-f", h("span", "", "Machine"), places.length ? chips : h("span", "dmuted", anywhere ? "No machine is online" : "No machine has this project yet"));
    const acts = h("div", "in-acts", go, button("Cancel", "quiet", () => { t.form = null; redraw(); }));
    const form = anywhere
      ? h("form", "cc-panel tm-form", h("b", "tm-title", "New terminal"), machine,
        input("cwd", "Folder", "Empty for the workspace or home folder"),
        input("command", "Command", "Empty for your shell"),
        input("name", "Name", "The command, or shell, when empty"), acts)
      : h("form", "cc-panel tm-form",
        input("command", "Command", "Empty for a shell, or npm run dev"),
        input("name", "Name", "The command, or shell, when empty"),
        machine,
        input("cwd", "Folder", "The project's folder"), acts);
    form.addEventListener("submit", e => { e.preventDefault(); go.click(); });
    return form;
  }

  /// The open terminal: its live screen, or the last lines of a view-only one.
  function openView(name: string, open: TerminalRow) {
    const t = of(name);
    const live = liveOf(name, open.id);
    const acts: HTMLElement[] = [];
    const items: MenuItem[] = [];
    const toggleTyping = () => {
      live.watching = !live.watching;
      live.screen.setInput(live.link === "attached" && !live.watching);
      if (!live.watching) live.screen.focus();
      redraw();
    };
    const detachHere = () => { detach(live); redraw(); };
    const attachHere = () => { live.tries = 0; attach(name, live); redraw(); };
    if (live.link === "attached" || live.link === "attaching") {
      const typing = button(live.watching ? "Take control" : "View only", "small", toggleTyping);
      typing.title = live.watching ? "Type into the terminal again" : "Stop typing into it, keep watching";
      const off = button("Detach", "small", detachHere);
      off.title = "Stop following it here; it keeps running";
      acts.push(typing, off);
      items.push({ label: live.watching ? "Take control" : "View only", on: toggleTyping }, { label: "Detach", on: detachHere });
    } else if (live.link === "detached" || (live.link === "ended" && open.state === "offline")) {
      acts.push(button("Attach", "small pri", attachHere));
      items.push({ label: "Attach", on: attachHere });
    }
    const running = open.state === "running";
    const stop = () => {
      if (!running) return void close(name, open.id);
      confirmSheet({
        title: `Stop ${open.name}?`,
        text: `It stops running on ${open.machine_name}.`,
        action: "Stop",
        run: () => close(name, open.id),
      });
    };
    acts.push(button(running ? "Kill…" : "Close", "small warn", stop));
    items.push({ label: running ? "Kill…" : "Close", warn: true, on: stop });
    const head = h("div", "tm-head",
      h("span", "", h("b", "", open.name), ` ${open.machine_name} · ${open.cwd}`),
      h("span", "tm-st", stateLabel(open)), ...acts);
    head.addEventListener("contextmenu", e => openMenu(e, open.name, items));
    let body: HTMLElement;
    if (live.link === "view_only") {
      const o = t.output;
      body = h("pre", "tm-screen", h("span", "tm-cmd", `${open.machine_name} ${open.cwd} $ ${open.command}`), "\n",
        o?.earlier ? "…\n" : "", (o ? o.lines : []).join("\n"));
      watch(name);
      // The screen follows its newest line.
      queueMicrotask(() => { const s = document.querySelector(".tm-screen"); if (s) s.scrollTop = s.scrollHeight; });
    } else {
      // A redraw moves the screen to the new page, which drops the focus:
      // the keys typed next would reach the window instead.
      const keep = live.screen.focused();
      body = live.screen.el;
      watch(null);
      requestAnimationFrame(() => live.screen.fit());
      if (keep) queueMicrotask(() => live.screen.focus());
    }
    const on = live.link === "attached" && open.state === "running";
    return h("div", "cc-panel tm-panel", head, body,
      h("div", "tm-foot", h("span", "", h("span", `ddot${on ? "" : " off"}`), " ", linkLabel(live.link, live.watching, open.machine_name, live.tries)),
        h("span", "", `started ${ago(open.started_at)}`)));
  }

  /// The full view: tabs, then the open terminal or the form.
  function panel(name: string, where: readonly Where[] | null | undefined) {
    if (shown !== name) { dropAll(); shown = name; }
    const t = of(name);
    if (t.list === undefined) load(name);
    const places = terminalPlaces(where);
    const tabs = h("div", "in-tabs tm-tabs", ...(t.list || []).map(x => {
      const b = button("", "tm-tab", () => select(name, x.id));
      b.append(h("span", `ddot${x.state === "running" ? "" : " off"}`), x.name);
      if (t.open === x.id) b.setAttribute("aria-current", "true");
      return b;
    }));
    const add = button("+", "tm-tab", () => {
      t.open = null;
      t.form = { command: "", name: "", machine: places[0]?.id || null, cwd: places[0]?.folder || "" };
      redraw();
    });
    add.setAttribute("aria-label", "New terminal");
    add.title = "New terminal";
    tabs.append(add);
    const out: HTMLElement[] = [tabs];
    if (t.error) out.push(h("div", "cc-err", t.error));
    if (t.list === null) {
      out.push(emptyPanel(h("b", "", "The server needs an update before it keeps terminals.")));
      watch(null);
      return out;
    }
    if (t.form) out.push(formView(name, places));
    const open = t.open && (t.list || []).find(x => x.id === t.open);
    if (open) {
      out.push(openView(name, open));
    } else {
      watch(null);
      if (!t.form) {
        out.push(emptyPanel(h("b", "", t.list?.length ? "Pick a terminal" : "No terminals yet"),
          button("New terminal", "pri", () => add.click())));
      }
    }
    return out;
  }

  /// Opens the New terminal form of the Terminals page on `machine`.
  function newOn(machine: string | null) {
    const t = of(ALL);
    t.open = null;
    t.form = { command: "", name: "", machine, cwd: "" };
    redraw();
  }

  /// The Terminals page: every machine with its terminals at the left, the
  /// open terminal or the New terminal form at the right.
  function machinesPanel(machines: readonly TerminalMachine[]) {
    if (shown !== ALL) { dropAll(); shown = ALL; }
    const t = of(ALL);
    if (t.list === undefined) load(ALL);
    if (t.list === null) {
      watch(null);
      return [emptyPanel(h("b", "", "The server needs an update before it lists terminals on every machine."))];
    }
    const groups = byMachine(machines, t.list || []);
    const places = machines.filter(m => m.online).map(m => ({ id: m.id, name: m.name, folder: "" }));
    const list = h("div", "tl-list", ...groups.map(m => {
      const add = button("New", "", () => newOn(m.id));
      add.disabled = !m.online;
      const head = h("h5", "", h("span", `ddot${m.online ? "" : " off"}`), m.online ? m.name : `${m.name} · offline`, add);
      const items = m.terms.map(x => {
        const b = h("button", "tl-it");
        b.type = "button";
        b.addEventListener("click", () => select(ALL, x.id));
        b.title = `${x.machine_name}: ${x.command} in ${x.cwd}`;
        b.append(h("span", `ddot${x.state === "running" ? "" : " off"}`), h("b", "", x.name),
          h("small", "", `${terminalWhere(x)} · ${terminalNote(x, ago)}`));
        if (t.open === x.id) b.setAttribute("aria-current", "true");
        return b;
      });
      const kids: Node[] = [head, ...(items.length ? items : [h("div", "tl-empty", "No terminals.")])];
      if (!m.online && items.length) kids.push(h("div", "tl-empty", "They keep running there and show again when it is back."));
      return h("div", "tl-m", ...kids);
    }));
    let right: HTMLElement;
    const open = t.open && (t.list || []).find(x => x.id === t.open);
    if (t.form) {
      watch(null);
      right = formView(ALL, places);
    } else if (open) {
      right = openView(ALL, open);
    } else {
      watch(null);
      const any = (t.list || []).length > 0;
      right = h("div", "cc-conn", h("b", "", any ? "Pick a terminal" : t.list ? "No terminals yet" : "Loading…"),
        button("New terminal", "pri", () => newOn(places[0]?.id || null)));
    }
    const out: HTMLElement[] = [];
    if (t.error) out.push(h("div", "cc-err", t.error));
    out.push(h("div", "tl", list, right));
    return out;
  }

  return {
    load,
    ensure,
    list: (name: string) => of(name).list,
    panel,
    machinesPanel,
    newOn,
    /// Opens terminal `id` on the Terminals page, attached live.
    show: (id: string) => select(ALL, id),
    stop: () => { watch(null); dropAll(); shown = null; },
  };
}
