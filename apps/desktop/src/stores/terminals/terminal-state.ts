// The terminals model: commands and shells kept running on the user's machines, apart from any
// session, like tmux sessions. The node owns them, so they keep running with the window closed
// and can be attached to again. terminals.tsx draws the Terminals page from makeTerminals.
import { createRef } from "react";
import { subscribeEach, topic } from "../../ui/mesh-events.ts";
import { pushedView } from "../../ui/snapshot-state.ts";
import { onShownChange } from "../../ui/active.ts";
import { RETRY_MS } from "../agents/reconnect-state.ts";
import type { invoke as Invoke, indexedChannel } from "../../ui/tauri.ts";
import { PageCache, type SectionStore } from "../../ui/pagecache.ts";
import { reason } from "../../ui/format.ts";
import type { Ago } from "../../ui/format.ts";
import { ALL } from "./terminal-list.ts";
import { newTermLike } from "./newkey-state.ts";

/// The view-only terminal's output while the Terminals page shows one.
export const terminalOutput = createRef<HTMLPreElement>();

export function makeTerminals(deps: TerminalDeps): Terminals {
  const { invoke, channel: streamChannel, makeScreen, ago, redraw } = deps;
  /// A line in the app's log: how a terminal's stream went.
  const log = (message: string, info: boolean) => { invoke("log_webview", { message, info }).catch(() => {}); };
  const all = new Map<string, ProjectTerminals>();
  /// Terminals this view started and has not attached to yet.
  const opened = new Set<string>();
  /// Live views by terminal id, for the project on show.
  const lives = new Map<string, Live>();
  let unfollow: (() => void) | null = null;
  let polling: string | null = null;
  let formVersion = 0;
  /// The project whose terminals view shows.
  let shown: string | null = null;

  // A hidden window lets go of its live views and attaches them again from
  // where each screen got to when it shows, so nothing is lost or repeated.
  if (typeof document !== "undefined") onShownChange(on => {
    for (const live of lives.values()) {
      if (!on && terminalFollowing(live.link)) detach(live, "paused");
      else if (on && live.link.kind === "paused") attach(ALL, live);
    }
  });

  const kept = deps.kept ?? new PageCache(null).section<TerminalRow[]>("terminals");

  /// A project's terminals, from the cache until they are read.
  const of = (name: string) => {
    let t = all.get(name);
    if (!t) all.set(name, t = { list: kept.get(name), open: null, output: null, form: null, error: "", busy: false });
    return t;
  };

  const loads = new Map<string, Promise<void>>();
  function load(name: string): Promise<void> {
    const pending = loads.get(name);
    if (pending) return pending;
    const read = readList(name).finally(() => loads.delete(name));
    loads.set(name, read);
    return read;
  }

  async function readList(name: string) {
    const t = of(name);

    try {
      t.list = name === ALL
        ? await invoke<TerminalRow[]>("terminals_all")
        : await invoke<TerminalRow[]>("terminals_list", { project: name });
      if (t.open && !t.list.some(x => x.id === t.open)) t.open = null;
      kept.put(name, t.list);
      t.error = "";
    } catch (e) {
      t.list ??= [];
      t.error = reason(e);
    }

    redraw();
  }

  const outputReads = new Map<string, Promise<void>>();
  async function readOutput(name: string) {
    const t = of(name);
    if (!t.open) return;
    const id = t.open;
    const previous = outputReads.get(id);
    if (previous) await previous;
    if (t.open !== id) return;
    let finish!: () => void;
    const current = new Promise<void>(done => { finish = done; });
    outputReads.set(id, current);

    try {
      const out = await invoke<OutputView>("terminal_output", { id });
      if (t.open !== id) return;
      t.output = out;
      setRow(name, out.terminal);
      t.error = "";
    } catch (e) {
      t.error = reason(e);
    } finally {
      if (outputReads.get(id) === current) outputReads.delete(id);
      finish();
    }

    redraw();
  }

  function setRow(name: string, row: TerminalRow) {
    const t = of(name);
    t.list = (t.list || []).map(x => (x.id === row.id ? row : x));
  }

  /// Takes every terminal the `terminals` push carries: each project's list is the rows of its own.
  function takeAll(rows: TerminalRow[]) {
    for (const [name, t] of all) {
      t.list = name === ALL ? rows : rows.filter(row => row.project === name);
      if (t.open && !t.list.some(x => x.id === t.open)) t.open = null;
      kept.put(name, t.list);
      t.error = "";
    }

    redraw();
  }

  subscribeEach(topic("terminals"), event => {
    const pushed = pushedView<TerminalRow[]>(event.data, "terminals_all");
    if (!pushed) return;
    if ("error" in pushed) { console.warn("terminals:", pushed.error); return; }

    takeAll(pushed.value);
  });

  /// Follows the open terminal's output of `name`, which its own topic pushes; null stops.
  function watch(name: string | null) {
    const id = name === null ? null : of(name).open;
    const key = name === null || !id ? null : `${name} ${id}`;
    if (polling === key) return;
    unfollow?.();
    unfollow = null;
    polling = key;
    if (name === null || !id) return;

    unfollow = subscribeEach(topic("terminal-output", id), event => {
      const t = of(name);
      const pushed = pushedView<OutputView>(event.data, "terminal_output", { id });
      if (!pushed || t.open !== id) return;
      if ("error" in pushed) { t.error = reason(pushed.error); redraw(); return; }

      t.output = pushed.value;
      setRow(name, pushed.value.terminal);
      t.error = "";
      redraw();
    });
  }

  /// The live view of terminal `id`, made and attached the first time.
  function liveOf(name: string, id: string) {
    let live = lives.get(id);

    if (!live) {
      const screen = makeScreen();
      live = { id, screen, link: { kind: "detached" }, next: 0, watching: false, view: newView(), sent: null, replay: new Replay(opened.delete(id)) };
      lives.set(id, live);
      const l = live;
      const input = (data: string) => {
        if (l.link.kind !== "attached" || l.watching) return;
        invoke("terminal_input", { id, view: l.view, data }).catch(e => { of(name).error = reason(e); redraw(); });
      };
      screen.onData(input);
      // The node does not answer queries yet: the screen's answers go as input.
      screen.onReply(input);
      screen.onResize(() => { if (l.link.kind === "attached") sendSize(l); });
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
  function attach(name: string, live: Live, tries = 0) {
    if (live.link.kind === "disposed") return;
    const attempt = { cancel: () => {} };
    changeTerminalLink(live, { kind: "attaching", attempt, tries });
    const current = () => terminalAttempt(live.link) === attempt;
    const show = (message: unknown) => {
      const ev = readTermMessage(message);
      if (ev === null) {
        log(`terminal ${live.id}: a stream message was not understood (${describeMessage(message)})`, false);
        return;
      }

      if (ev.kind === "attached") {
        log(`terminal ${live.id}: attached`, true);
        live.replay.start(performance.now());
        changeTerminalLink(live, { kind: "attached", attempt });
        live.screen.setInput(!live.watching);
        // The screen may have been fitted while it attached.
        sendSize(live);
        redraw();
      } else if (ev.kind === "output") {
        const bytes = ev.data;
        const { skip, gap } = splice(live.next, ev.at, bytes.length);
        if (gap) live.screen.note("… earlier output was not kept");
        if (live.replay.is(performance.now())) live.screen.replay(bytes.subarray(skip));
        else live.screen.write(bytes.subarray(skip));
        live.next = Math.max(live.next, ev.at + bytes.length);
      } else if (ev.kind === "ended") {
        log(`terminal ${live.id}: ended (${ev.state})`, true);
        changeTerminalLink(live, { kind: "ended" });
        live.screen.setInput(false);
        // Its shell exited: the terminal goes from the list and its view
        // closes. One whose machine went away may still run there.
        if (endsForGood(ev.state)) return forget(name, live.id);
        const row = (of(name).list || []).find(x => x.id === live.id);
        if (row) setRow(name, { ...row, state: ev.state, code: ev.code });
        redraw();
      } else {
        log(`terminal ${live.id}: view only`, true);
        changeTerminalLink(live, { kind: "view_only" });
        live.screen.setInput(false);
        readOutput(name);
        redraw();
      }
    };
    const stream = inOrderChannel(streamChannel, current, show, missing => {
      log(`terminal ${live.id}: stream message ${missing} was lost; attaching again`, false);
      attach(name, live, live.link.kind === "attaching" ? live.link.tries : 0);
    });
    attempt.cancel = stream.cancel;
    // The attach carries the size, so the node has it from here on.
    const size = live.screen.size();
    live.sent = size;
    invoke("terminal_attach", { id: live.id, view: live.view, from: live.next, ...size, onEvent: stream.channel })
      .then(() => { if (current()) retry(name, live, ""); })
      .catch(e => {
        if (!current()) return;
        const why = reason(e);

        if (/needs an update/i.test(why)) {
          changeTerminalLink(live, { kind: "view_only" });
          live.screen.setInput(false);
          readOutput(name);
          redraw();
          return;
        }

        retry(name, live, why);
      });
  }

  function retry(name: string, live: Live, why: string) {
    const tries = (live.link.kind === "attaching" ? live.link.tries : 0) + 1;
    log(`terminal ${live.id}: stream dropped, try ${tries}${why ? `: ${why}` : ""}`, true);
    live.screen.setInput(false);
    changeTerminalLink(live, { kind: "retrying", tries, timer: setTimeout(() => attach(name, live, tries), RETRY_MS) });
    redraw();
  }

  function detach(live: Live, kind: "detached" | "paused" | "disposed" = "detached") {
    const following = terminalFollowing(live.link);
    changeTerminalLink(live, { kind });
    live.screen.setInput(false);
    if (following) invoke("terminal_detach", { id: live.id, view: live.view }).catch(() => {});
  }

  /// Detaches and drops every live view, such as when the view closes.
  function dropAll() {
    for (const live of lives.values()) {
      detach(live, "disposed");
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
    const fields = { project: form.project ?? name, machineId: form.machine, name: form.name, command: form.command, cwd: form.cwd };

    try {
      const made = await invoke<TerminalRow>("terminal_open", { terminal: { ...fields, ...START_SIZE } });
      t.list = [...(t.list || []), made];
      t.form = null;
      opened.add(made.id);
      select(name, made.id);
    } catch (e) {
      t.error = reason(e);
    } finally {
      t.busy = false;
    }

    redraw();
  }

  /// Drops ended terminal `id` here and on its machine.
  function forget(name: string, id: string) {
    const live = lives.get(id);

    if (live) {
      changeTerminalLink(live, { kind: "disposed" });
      live.screen.dispose();
      lives.delete(id);
    }

    const t = of(name);
    t.list = (t.list || []).filter(x => x.id !== id);
    if (t.open === id) { t.open = null; t.output = null; }
    // Its row on the machine; the next listing drops it too when this fails.
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
        detach(live, "disposed");
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

  /// Opens the New terminal form of the Terminals page on `machine`.
  function newOn(machine: string | null) {
    const t = of(ALL);
    t.open = null;
    formVersion++;
    t.form = { command: "", name: "", machine, cwd: "" };
    redraw();
  }

  /// ⌘N on the Terminals page: a new terminal on the open one's machine, in
  /// its folder and project. False when none is open.
  function newLikeOpen() {
    const t = of(ALL);
    const open = t.open ? (t.list || []).find(x => x.id === t.open) : undefined;
    const like = newTermLike(open ? [{ ...open, focused: !!lives.get(open.id)?.screen.focused() }] : []);
    if (!like) return false;
    if (!t.busy) start(ALL, { command: "", name: "", machine: like.machine_id, cwd: like.cwd, project: like.project });
    return true;
  }

  /// Focuses the selected terminal without attaching it or changing typing mode.
  function focusOpen() {
    const t = of(ALL);
    if (shown !== ALL || t.form || !t.open) return false;
    const live = lives.get(t.open);
    if (!live) return false;

    if (live.link.kind === "view_only") {
      const output = terminalOutput.current;
      if (!output?.isConnected) return false;
      output.focus();
    } else {
      live.screen.focus();
    }

    return true;
  }

  return {
    load,
    list: (name: string) => of(name).list,
    follow: watch,
    newOn,
    newLikeOpen,
    focusOpen,
    /// Opens terminal `id` on the Terminals page, attached live.
    show: (id: string) => select(ALL, id),
    /// Closes the open terminal or form on the Terminals page, back to its list.
    unselect: () => { const t = of(ALL); t.open = null; t.form = null; redraw(); },
    stop: () => { watch(null); dropAll(); shown = null; },
    /// What the Terminals page draws with: the page's state and the actions its panel takes.
    page: {
      of, liveOf, start, close, detach, attach, watch, select, load, newOn, focusOpen, redraw, ago,
      /// Shows the Terminals page: it drops the live views of any other.
      enter: () => { if (shown !== ALL) { dropAll(); shown = ALL; } },
      form: () => formVersion,
    },
  };
}

/// The terminals model: what the lists and the sidebar read, and what the Terminals page draws with.
export interface Terminals {
  load(name: string): Promise<void>;
  list(name: string): TerminalRow[] | undefined;
  follow(name: string | null): void;
  newOn(machine: string | null): void;
  newLikeOpen(): boolean;
  focusOpen(): boolean;
  /// Opens terminal `id` on the Terminals page, attached live.
  show(id: string): void;
  /// Closes the open terminal or form on the Terminals page, back to its list.
  unselect(): void;
  stop(): void;
  /// What the Terminals page draws with: the page's state and the actions its panel takes.
  page: {
    of(name: string): ProjectTerminals;
    liveOf(name: string, id: string): Live;
    start(name: string, form: TerminalForm): Promise<void>;
    close(name: string, id: string): Promise<void>;
    detach(live: Live, kind?: "detached" | "paused" | "disposed"): void;
    attach(name: string, live: Live, tries?: number): void;
    watch(name: string | null): void;
    select(name: string, id: string): void;
    load(name: string): Promise<void>;
    newOn(machine: string | null): void;
    focusOpen(): boolean;
    redraw(): void;
    ago: Ago;
    /// Shows the Terminals page: it drops the live views of any other.
    enter(): void;
    form(): number;
  };
}

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

/// What an attached terminal sends: output from byte `at`, its end, or
/// that it takes no input.
export type TermEvent =
  | { kind: "attached" }
  | { kind: "output"; at: number; data: Uint8Array }
  | TermNotice;

/// The events other than output, as JSON on the channel (TermNotice).
type TermNotice =
  | { kind: "attached" }
  | { kind: "ended"; state: string; code: number | null }
  | { kind: "view_only" };

/// What `terminal_attach`'s channel carries: output as raw bytes, the
/// offset first as eight little-endian bytes, and notices as JSON. Tauri
/// hands raw bytes over as an ArrayBuffer, or as an array of numbers when its
/// IPC falls back to postMessage.
export type TermMessage = ArrayBuffer | ArrayBufferView | number[] | TermNotice;

/// A channel message as an event, or null for one this page does not know.
export function readTermMessage(message: unknown): TermEvent | null {
  const bytes = rawBytes(message);
  if (bytes !== null) {
    if (bytes.length < 8) return null;
    const at = Number(new DataView(bytes.buffer, bytes.byteOffset, 8).getBigUint64(0, true));
    return { kind: "output", at, data: bytes.subarray(8) };
  }

  if (typeof message !== "object" || message === null || !("kind" in message)) return null;
  switch (message.kind) {
    case "attached":
      return { kind: "attached" };
    case "view_only":
      return { kind: "view_only" };
    case "ended": {
      const ended = message as { state?: unknown; code?: unknown };
      if (typeof ended.state !== "string") return null;
      return { kind: "ended", state: ended.state, code: typeof ended.code === "number" ? ended.code : null };
    }
    default:
      return null;
  }
}

function rawBytes(message: unknown): Uint8Array | null {
  if (message instanceof ArrayBuffer) return new Uint8Array(message);
  if (ArrayBuffer.isView(message)) return new Uint8Array(message.buffer, message.byteOffset, message.byteLength);
  if (Array.isArray(message) && message.every(n => typeof n === "number")) return Uint8Array.from(message);
  return null;
}

/// What a channel message was, for the log when it is not understood.
export function describeMessage(message: unknown): string {
  if (message === null) return "null";
  if (Array.isArray(message)) return `array of ${message.length}`;
  if (typeof message !== "object") return typeof message;
  const name = Object.getPrototypeOf(message)?.constructor?.name ?? "object";
  return name === "Object" ? `object with keys ${Object.keys(message).join(", ")}` : name;
}

/// A machine a terminal can start on, in the project's folder there.
export interface TerminalPlace {
  id: string;
  name: string;
  folder: string;
}

/// A screen whose terminal comes from `load` when first made, so the terminal stays out
/// of the window's first load. What it is given before then waits.
export function lazyScreen(load: () => Promise<(el: HTMLElement) => Screen>): Screen {
  const el = document.createElement("div");
  el.className = "tm-term";
  let real: Screen | null = null;
  let gone = false;
  const early: ((screen: Screen) => void)[] = [];
  const use = (fn: (screen: Screen) => void) => { if (real) fn(real); else early.push(fn); };
  void load().then(make => {
    if (gone) return;
    const screen = real = make(el);
    for (const fn of early.splice(0)) fn(screen);
  }, e => console.warn("terminal screen:", e));

  return {
    el,
    write: data => use(screen => screen.write(data)),
    replay: data => use(screen => screen.replay(data)),
    note: text => use(screen => screen.note(text)),
    size: () => real?.size() ?? { cols: 80, rows: 24 },
    fit: () => use(screen => screen.fit()),
    // Asked before the terminal loaded: not if the keyboard went elsewhere meanwhile.
    focus: () => {
      const from = document.activeElement;
      use(screen => { if (document.activeElement === from) screen.focus(); });
    },
    focused: () => el.contains(document.activeElement),
    onData: cb => use(screen => screen.onData(cb)),
    onReply: cb => use(screen => screen.onReply(cb)),
    onResize: cb => use(screen => screen.onResize(cb)),
    onEscape: cb => use(screen => screen.onEscape(cb)),
    setInput: on => use(screen => screen.setInput(on)),
    dispose: () => { gone = true; early.length = 0; real?.dispose(); },
  };
}

/// A terminal emulator on the page (termview.ts runs one with Ghostty).
export interface Screen {
  el: HTMLElement;
  write(data: Uint8Array): void;
  /// Output from before this view attached: shown, but queries in it are
  /// not answered, since whoever asked them is long gone.
  replay(data: Uint8Array): void;
  /// A dim line of our own, such as "… earlier output was not kept".
  note(text: string): void;
  size(): { cols: number; rows: number };
  /// Fits the screen to its element, once it is on the page.
  fit(): void;
  focus(): void;
  /// Whether the keyboard is in it now.
  focused(): boolean;
  /// What the user typed, pasted or clicked, for the program.
  onData(cb: (data: string) => void): void;
  /// What the terminal answers the program's queries with (cursor position,
  /// device attributes, image support). Apart from onData, so a node that
  /// answers queries itself can have these dropped.
  onReply(cb: (data: string) => void): void;
  onResize(cb: (size: { cols: number; rows: number }) => void): void;
  /// Esc typed in it, which it does not send on; only a pane's terminal sets this.
  /// A program on the alternate screen (vim, less) still gets Esc.
  onEscape(cb: () => void): void;
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

/// What the window hands `makeTerminals`.
export interface TerminalDeps {
  invoke: typeof Invoke;
  channel: typeof indexedChannel;
  makeScreen: () => Screen;
  ago: Ago;
  redraw: () => void;
  /// Where each project's list is kept between windows; in memory only
  /// without it.
  kept?: SectionStore<TerminalRow[]>;
}

/// The New terminal form as typed so far.
export interface TerminalForm {
  command: string;
  name: string;
  machine: string | null;
  cwd: string;
  /// The project it is listed under, when not the page's.
  project?: string;
}

/// How an open terminal's live view stands.
type Link = "attaching" | "attached" | "detached" | "ended" | "view_only";

interface TerminalAttempt { cancel(): void }

export type TerminalLink =
  | { kind: "attaching"; attempt: TerminalAttempt; tries: number }
  | { kind: "attached"; attempt: TerminalAttempt }
  | { kind: "retrying"; timer: ReturnType<typeof setTimeout>; tries: number }
  | { kind: "closing"; completion: Promise<void> }
  | { kind: "paused" | "detached" | "ended" | "view_only" | "disposed" };

export const terminalAttempt = (link: TerminalLink) => "attempt" in link ? link.attempt : null;
export const terminalFollowing = (link: TerminalLink) => terminalAttempt(link) !== null || link.kind === "retrying";

/// Leaves the old attempt and its timers before a different stream can start.
export function changeTerminalLink(target: { link: TerminalLink }, link: TerminalLink) {
  const before = target.link, attempt = terminalAttempt(before);
  target.link = link;
  if (before.kind === "retrying") clearTimeout(before.timer);
  if (attempt && attempt !== terminalAttempt(link)) attempt.cancel();
}

export function viewLink(link: TerminalLink): Link {
  if (link.kind === "retrying") return "attaching";
  if (link.kind === "paused" || link.kind === "closing" || link.kind === "disposed") return "detached";
  return link.kind;
}

/// The open terminal's live view: its screen and how far its output got.
export interface Live {
  id: string;
  screen: Screen;
  link: TerminalLink;
  /// Bytes of output on the screen so far: where a reattach starts.
  next: number;
  /// Typing is off by choice ("View only").
  watching: boolean;
  /// This view's name for its stream.
  view: string;
  /// The size the node last got from this view.
  sent: TermSize | null;
  /// Which output replays what the node kept.
  replay: Replay;
}

/// One project's terminals: `list` is undefined while it loads.
export interface ProjectTerminals {
  list: TerminalRow[] | undefined;
  open: string | null;
  output: OutputView | null;
  form: TerminalForm | null;
  error: string;
  busy: boolean;
}

/// The size a terminal starts at, before its screen is fitted.
const START_SIZE = { cols: 100, rows: 30 };
/// How long a stream message may stay missing while later ones came: then
/// it was lost, and the screen attaches again from what it showed.
export const LOST_MS = 2000;

/// Where a chunk of `len` bytes from byte `at` meets the `next` byte the
/// screen waits for: `skip` leading bytes it already shows, and `gap` when
/// bytes before it were no longer kept.
export function splice(next: number, at: number, len: number): { skip: number; gap: boolean } {
  if (at > next) return { skip: 0, gap: true };
  return { skip: Math.min(next - at, len), gap: false };
}

/// How long after attaching output still counts as the node's replay of what
/// it kept, at most, and the pause in output that ends the replay sooner.
const REPLAY_MAX_MS = 2000;
const REPLAY_GAP_MS = 150;

/// Which output on a stream replays what the node kept from before the view
/// attached. The node sends that at once; the protocol does not say where it
/// ends, so a pause in the output or REPLAY_MAX_MS does. A terminal the view
/// itself just opened has nothing to replay: its first queries are live.
export class Replay {
  private until = 0;
  private last = 0;
  private opened: boolean;

  constructor(opened: boolean) {
    this.opened = opened;
  }

  /// The stream attached at `now`.
  start(now: number) {
    this.until = this.opened ? 0 : now + REPLAY_MAX_MS;
    this.opened = false;
    this.last = now;
  }

  /// Whether output arriving at `now` is still the replay.
  is(now: number): boolean {
    if (now > this.until || now - this.last > REPLAY_GAP_MS) {
      this.until = 0;
      return false;
    }

    this.last = now;
    return true;
  }
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

/// A stream's messages back in order by their index.
class InOrder {
  private next = 0;
  private readonly held = new Map<number, unknown>();

  /// Message `index`, and the messages that are now in order.
  take(message: unknown, index: number): unknown[] {
    if (index < this.next) return [];
    this.held.set(index, message);
    const ready: unknown[] = [];

    while (this.held.has(this.next)) {
      ready.push(this.held.get(this.next));
      this.held.delete(this.next);
      this.next++;
    }

    return ready;
  }

  /// The index of the message later ones wait for, or null when none wait.
  missing(): number | null {
    return this.held.size > 0 ? this.next : null;
  }
}

/// A terminal stream's channel: its messages go to `show` in order while
/// `current`. One missing for `LOST_MS` while later ones came was lost:
/// `lost` gets its index, and the screen should follow again from what it
/// showed, since the rest would never come.
export function inOrderChannel(
  open: typeof indexedChannel,
  current: () => boolean,
  show: (message: unknown) => void,
  lost: (index: number) => void,
) {
  const order = new InOrder();
  let timer: ReturnType<typeof setTimeout> | undefined;
  let waiting: number | null = null;
  /// Times the message waited for, from when it was first waited for.
  const watchGap = () => {
    const missing = order.missing();
    if (missing === waiting) return;

    clearTimeout(timer);
    waiting = missing;
    if (missing === null) return;
    timer = setTimeout(() => { if (current()) lost(missing); }, LOST_MS);
  };

  const channel = open((message, index) => {
    if (!current()) return;

    for (const next of order.take(message, index)) {
      if (!current()) return;
      show(next);
    }
    if (current()) watchGap();
  });

  return { channel, cancel: () => clearTimeout(timer) };
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
    case "view_only": return "View only: the last 100 lines";
  }
}
