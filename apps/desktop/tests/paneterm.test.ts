import { answers, calls, callbacks, nativeEvent } from "./bar-env.ts";
import test from "node:test";
import assert from "node:assert/strict";
import { attach, dropTerm, paneTerm, termStatus } from "../src/agentpane/terminal.ts";
import { openTabs, closeTab, closePaneTabs, stopTerminalTab, type MediaTab } from "../src/agentpane/media.ts";
import { paneModel } from "../src/agentpane/media-layout.ts";
import { ALL } from "../src/terminals/terminal-list.ts";
import { makeTerminals, LOST_MS, type Screen, type TermMessage, type TerminalRow } from "../src/terminals/terminals.tsx";
import { indexedChannel } from "../src/ui/tauri.ts";
import { RETRY_MS } from "../src/agents/reconnect-state.ts";

const setTabs = (tabs: MediaTab[]) => paneModel.setState({ tabs });
const addTabs = (...tabs: MediaTab[]) => paneModel.setState(s => ({ tabs: [...s.tabs, ...tabs] }));


const ROW: TerminalRow = {
  id: "t1", project: "", machine_id: "m1", machine_name: "build-box", name: "shell",
  command: "", cwd: "/srv/project", state: "running", code: null, started_at: 1,
};

/// A screen that keeps what reaches it, at a size the test sets.
function fakeScreen(cols = 100, rows = 30) {
  const shown: string[] = [];
  const replayed: string[] = [];
  let resized: (size: { cols: number; rows: number }) => void = () => {};
  let escaped: () => void = () => {};
  const screen: Screen & { shown: string[]; replayed: string[]; resize(cols: number, rows: number): void; pressEsc(): void } = {
    el: {} as HTMLElement,
    shown,
    replayed,
    write: data => shown.push(new TextDecoder().decode(data)),
    // Shown like the rest; kept apart too, as its queries go unanswered.
    replay: data => { const text = new TextDecoder().decode(data); shown.push(text); replayed.push(text); },
    note: text => shown.push(text),
    size: () => ({ cols, rows }),
    fit() {},
    focus() {},
    focused: () => false,
    onData() {},
    onReply() {},
    onResize: cb => { resized = cb; },
    onEscape: cb => { escaped = cb; },
    pressEsc: () => escaped(),
    setInput() {},
    dispose() {},
    resize(c, r) { cols = c; rows = r; resized({ cols, rows }); },
  };
  return screen;
}

/// Output as the channel carries it: the offset, then the bytes.
function output(at: number, text: string): ArrayBuffer {
  const data = new TextEncoder().encode(text);
  const message = new Uint8Array(8 + data.length);
  new DataView(message.buffer).setBigUint64(0, BigInt(at), true);
  message.set(data, 8);
  return message.buffer;
}

/// The attach the next `terminal_attach` starts: its event channel, and
/// how to end its stream.
function nextAttach() {
  let end!: () => void;
  answers.terminal_attach = new Promise<null>(resolve => { end = () => resolve(null); });
  let index = 0;
  return {
    end,
    /// Message `ev`, with the next index or `at` one Tauri gives.
    send(ev: TermMessage, at = index) {
      index = at + 1;
      const call = calls.findLast(c => c.cmd === "terminal_attach")!;
      const channel = (call.args as { onEvent: { id: number } }).onEvent;
      callbacks.get(channel.id)?.({ message: ev, index: at });
    },
  };
}

const settle = () => new Promise<void>(resolve => setImmediate(resolve));
const count = (cmd: string) => calls.filter(c => c.cmd === cmd).length;

test("a dropped stream reconnects in the footer, not on the screen", async t => {
  t.mock.timers.enable({ apis: ["setTimeout"] });
  calls.length = 0;
  const screen = fakeScreen();
  const term = paneTerm(ROW, screen, { onEnd() {}, onLink() {}, onEscape() {} });
  const first = nextAttach();
  attach(term);
  first.send({ kind: "attached" });
  first.send(output(0, "% "));
  const second = nextAttach();
  first.end();
  await settle();
  assert.equal(termStatus(term, "build-box"), "Reconnecting to build-box…");
  assert.deepEqual(screen.shown, ["% "], "nothing of ours was written to the screen");
  t.mock.timers.tick(RETRY_MS);
  assert.equal(count("terminal_attach"), 2);
  assert.equal((calls.at(-1)!.args as { from: number }).from, 2, "it picks up after what the screen shows");
  // The node resends from where the screen got to; nothing shows twice.
  second.send({ kind: "attached" });
  second.send(output(0, "% ls\r\n"));
  assert.deepEqual(screen.shown, ["% ", "ls\r\n"]);
  assert.equal(termStatus(term, "build-box"), "build-box · what you type goes there");
});

test("attaching and reconnecting do not take keyboard focus", () => {
  calls.length = 0;
  const screen = fakeScreen();
  let focuses = 0;
  screen.focus = () => { focuses++; };
  const term = paneTerm(ROW, screen, { onEnd() {}, onLink() {}, onEscape() {} });

  for (let i = 0; i < 2; i++) {
    const stream = nextAttach();
    attach(term);
    stream.send({ kind: "attached" });
    assert.equal(focuses, 0);
    assert.equal(term.link.kind, "attached");
  }
});

test("an exited or missing shell closes its tab instead of reconnecting", async t => {
  t.mock.timers.enable({ apis: ["setTimeout"] });

  for (const state of ["exited", "gone"]) {
    calls.length = 0;
    let ended = 0;
    const term = paneTerm(ROW, fakeScreen(), { onEnd: () => { ended++; }, onLink() {}, onEscape() {} });
    const stream = nextAttach();
    attach(term);
    stream.send({ kind: "attached" });
    stream.send({ kind: "ended", state, code: state === "exited" ? 0 : null });
    stream.end();
    await settle();
    t.mock.timers.tick(60_000);

    assert.equal(ended, 1);
    assert.equal(term.link.kind, "ended");
    assert.equal(count("terminal_attach"), 1);
  }
});

test("Esc typed in the screen reaches the tab's hook, not the shell", () => {
  calls.length = 0;
  const screen = fakeScreen();
  let left = 0;
  paneTerm(ROW, screen, { onEnd() {}, onLink() {}, onEscape: () => { left++; } });
  screen.pressEsc();
  assert.equal(left, 1);
  assert.equal(count("terminal_input"), 0, "nothing was sent to the terminal");
});

test("a stream that a newer attach replaced ending does not reconnect", async t => {
  t.mock.timers.enable({ apis: ["setTimeout"] });
  calls.length = 0;
  const term = paneTerm(ROW, fakeScreen(), { onEnd() {}, onLink() {}, onEscape() {} });
  const first = nextAttach();
  attach(term);
  const second = nextAttach();
  attach(term);
  second.send({ kind: "attached" });
  first.end();
  await settle();
  assert.equal(term.link.kind, "attached");
  t.mock.timers.tick(60_000);
  assert.equal(count("terminal_attach"), 2);
  second.end();
  await settle();
});

test("the Terminals page reconnects without writing to the screen", async t => {
  t.mock.timers.enable({ apis: ["setTimeout"] });
  calls.length = 0;
  const screen = fakeScreen();
  answers.terminals_all = [ROW];
  const terms = makeTerminals({
    invoke: (async (cmd: string, args?: Record<string, unknown>) => {
      calls.push({ cmd, args });
      return answers[cmd] ?? null;
    }) as never,
    channel: indexedChannel,
    makeScreen: () => screen,
    ago: () => "",
    redraw() {},
  });
  await terms.load(ALL);
  const first = nextAttach();
  terms.show("t1");
  first.send({ kind: "attached" });
  assert.equal(count("terminal_resize"), 0);
  nextAttach();
  first.end();
  await settle();
  t.mock.timers.tick(RETRY_MS);
  assert.equal(count("terminal_attach"), 2);
  assert.deepEqual(screen.shown, []);
  terms.stop();
});

/// The session's pane is not on screen: closing a tab draws nothing.
function noPane(t: { after(fn: () => void): void }) {
  const doc = document as { getElementById(id: string): unknown };
  const was = doc.getElementById;
  doc.getElementById = () => null;
  t.after(() => { doc.getElementById = was; });
}

test("closing a terminal's tab ends the terminal on its machine", async t => {
  noPane(t);
  calls.length = 0;
  const term = paneTerm(ROW, fakeScreen(), { onEnd() {}, onLink() {}, onEscape() {} });
  const attached = nextAttach();
  attach(term);
  attached.send({ kind: "attached" });
  const tab: MediaTab = { id: "term:shell 1", kind: "term", name: "shell 1", path: null, url: null, size: 0, started: 0, done: 0, error: "", rev: 0, viewHidden: false, note: null, player: null, playerAt: 0, term };
  addTabs(tab);
  closeTab(tab.id);
  await settle();
  assert.deepEqual(calls.filter(c => c.cmd === "terminal_close").map(c => c.args), [{ id: "t1" }]);
  assert.ok(!openTabs().includes(tab), "the tab went");
  attached.end();
  await settle();
  assert.equal(count("terminal_attach"), 1, "a stopped terminal is not followed again");
});

test("a terminal whose stop fails keeps its tab and is followed again", async t => {
  noPane(t);
  calls.length = 0;
  const term = paneTerm(ROW, fakeScreen(), { onEnd() {}, onLink() {}, onEscape() {} });
  const attached = nextAttach();
  attach(term);
  attached.send({ kind: "attached" });
  const tab: MediaTab = { id: "term:shell 2", kind: "term", name: "shell 2", path: null, url: null, size: 0, started: 0, done: 0, error: "", rev: 0, viewHidden: false, note: null, player: null, playerAt: 0, term };
  addTabs(tab);
  answers.terminal_close = Promise.reject(new Error("build-box: the node stopped answering"));
  nextAttach();
  closeTab(tab.id);
  await settle();
  delete answers.terminal_close;
  assert.ok(openTabs().includes(tab), "the tab stays");
  assert.equal(term.link.kind, "attaching");
  assert.equal(count("terminal_attach"), 2);
  setTabs([]);
});

test("repeated closes wait for the same terminal stop", async t => {
  noPane(t);
  calls.length = 0;
  let stopped = () => {};
  answers.terminal_close = new Promise<void>(resolve => { stopped = resolve; });
  t.after(() => { delete answers.terminal_close; });
  const term = paneTerm(ROW, fakeScreen(), { onEnd() {}, onLink() {}, onEscape() {} });
  const tab: MediaTab = { id: "term:shell", kind: "term", name: "shell", path: null, url: null, size: 0, started: 0, done: 0, error: "", rev: 0, viewHidden: false, note: null, player: null, playerAt: 0, term };
  addTabs(tab);

  const first = stopTerminalTab(tab), second = stopTerminalTab(tab);
  await settle();
  assert.equal(term.link.kind, "closing");
  assert.equal(count("terminal_close"), 1);
  assert.ok(openTabs().includes(tab));

  stopped();
  await Promise.all([first, second]);
  assert.ok(!openTabs().includes(tab));
});


test("leaving a session detaches its terminals and drops their screens", async t => {
  noPane(t);
  calls.length = 0;
  let disposed = 0;
  const screen = fakeScreen();
  screen.dispose = () => { disposed++; };
  const term = paneTerm(ROW, screen, { onEnd() {}, onLink() {}, onEscape() {} });
  const stream = nextAttach();
  attach(term);
  stream.send({ kind: "attached" });
  addTabs({ id: "term:shell", kind: "term", name: "shell", path: null, url: null, size: 0, started: 0, done: 0, error: "", rev: 0, viewHidden: false, note: null, player: null, playerAt: 0, term });
  closePaneTabs();
  await settle();
  assert.equal(openTabs().length, 0);
  assert.equal(disposed, 1);
  assert.equal(count("terminal_close"), 0);
  stream.end();
  await settle();
  assert.equal(count("terminal_attach"), 1);
});

test("leaving keeps starting and existing terminals alive", async t => {
  noPane(t);
  calls.length = 0;
  const starting: MediaTab = { id: "term:new", kind: "term", name: "new", path: null, url: null, size: 0, started: 0, done: 0, error: "", rev: 0, viewHidden: false, note: null, player: null, playerAt: 0 };
  addTabs(starting, { ...starting, id: "term:existing", existing: ROW });
  closePaneTabs();
  await settle();
  assert.equal(starting.closed, undefined);
  assert.equal(openTabs().length, 0);
  assert.equal(count("terminal_close"), 0);
});

test("leaving during a tab close does not send a second stop", async t => {
  noPane(t);
  calls.length = 0;
  let stopped!: () => void;
  answers.terminal_close = new Promise<void>(resolve => { stopped = resolve; });
  const term = paneTerm(ROW, fakeScreen(), { onEnd() {}, onLink() {}, onEscape() {} });
  addTabs({ id: "term:shell", kind: "term", name: "shell", path: null, url: null, size: 0, started: 0, done: 0, error: "", rev: 0, viewHidden: false, note: null, player: null, playerAt: 0, term });
  closeTab("term:shell");
  closePaneTabs();
  stopped();
  await settle();
  delete answers.terminal_close;
  assert.equal(count("terminal_close"), 1);
  assert.equal(openTabs().length, 0);
});

test("a stop refused after leaving cannot reconnect the disposed terminal", async t => {
  noPane(t);
  calls.length = 0;
  let refuse!: (e: Error) => void;
  answers.terminal_close = new Promise<void>((_, reject) => { refuse = reject; });
  const screen = fakeScreen();
  let disposed = 0;
  screen.dispose = () => { disposed++; };
  const term = paneTerm(ROW, screen, { onEnd() {}, onLink() {}, onEscape() {} });
  const stream = nextAttach();
  attach(term);
  stream.send({ kind: "attached" });
  addTabs({ id: "term:shell", kind: "term", name: "shell", path: null, url: null, size: 0, started: 0, done: 0, error: "", rev: 0, viewHidden: false, note: null, player: null, playerAt: 0, term });
  closeTab("term:shell");
  closePaneTabs();
  refuse(new Error("the node stopped answering"));
  await settle();
  delete answers.terminal_close;
  assert.equal(term.link.kind, "disposed");
  assert.equal(count("terminal_attach"), 1);
  assert.equal(disposed, 1);
  assert.equal(openTabs().length, 0);
});

/// Sends `ev` down the latest stream of view `view`.
function sendTo(view: string, ev: TermMessage) {
  const call = calls.findLast(c => c.cmd === "terminal_attach" && (c.args as { view: string }).view === view)!;
  const id = (call.args as { onEvent: { id: number } }).onEvent.id;
  const index = sent.get(id) ?? 0;
  sent.set(id, index + 1);
  callbacks.get(id)?.({ message: ev, index });
}

/// The next message index of each stream sendTo writes to.
const sent = new Map<number, number>();

test("a terminal hidden then shown again shows the output from meanwhile, in order", () => {
  nativeEvent("bar-shown");
  calls.length = 0;
  nextAttach();
  const screen = fakeScreen();
  const term = paneTerm(ROW, screen, { onEnd() {}, onLink() {}, onEscape() {} });
  const mine = (cmd: string) => calls.filter(c => c.cmd === cmd && (c.args as { view: string }).view === term.view);
  attach(term);
  sendTo(term.view, { kind: "attached" });
  sendTo(term.view, output(0, "one\r\n"));

  nativeEvent("bar-hidden");
  assert.equal(mine("terminal_detach").length, 1, "a hidden pane lets go of the stream");
  sendTo(term.view, output(5, "two\r\n"));
  assert.deepEqual(screen.shown, ["one\r\n"], "nothing is drawn while hidden");

  nativeEvent("bar-shown");
  assert.equal(mine("terminal_attach").length, 2);
  assert.equal((mine("terminal_attach").at(-1)!.args as { from: number }).from, 5, "it picks up after what the screen shows");
  sendTo(term.view, { kind: "attached" });
  sendTo(term.view, output(0, "one\r\ntwo\r\nthree\r\n"));
  assert.deepEqual(screen.shown, ["one\r\n", "two\r\nthree\r\n"]);
  assert.equal(term.link.kind, "attached");
  dropTerm(term);
});

test("the Terminals page lets go while the window is hidden and picks up after", async () => {
  nativeEvent("center-shown");
  calls.length = 0;
  nextAttach();
  const screen = fakeScreen();
  answers.terminals_all = [ROW];
  const terms = makeTerminals({
    invoke: (async (cmd: string, args?: Record<string, unknown>) => {
      calls.push({ cmd, args });
      return answers[cmd] ?? null;
    }) as never,
    channel: indexedChannel,
    makeScreen: () => screen,
    ago: () => "",
    redraw() {},
  });
  await terms.load(ALL);
  terms.show("t1");
  const view = (calls.findLast(c => c.cmd === "terminal_attach")!.args as { view: string }).view;
  sendTo(view, { kind: "attached" });
  sendTo(view, output(0, "a"));

  nativeEvent("center-hidden");
  const mine = (cmd: string) => calls.filter(c => c.cmd === cmd && (c.args as { view: string }).view === view);
  assert.equal(mine("terminal_detach").length, 1);
  nativeEvent("center-shown");
  assert.equal((mine("terminal_attach").at(-1)!.args as { from: number }).from, 1);
  sendTo(view, output(0, "ab"));
  assert.deepEqual(screen.shown, ["a", "b"]);
  terms.stop();
});

test("stream messages show in order, and a lost one attaches again from what showed", async t => {
  t.mock.timers.enable({ apis: ["setTimeout"] });
  calls.length = 0;
  const screen = fakeScreen();
  const term = paneTerm(ROW, screen, { onEnd() {}, onLink() {}, onEscape() {} });
  const first = nextAttach();
  attach(term);

  // Tauri fetches large messages, so a later one can come first.
  first.send({ kind: "attached" }, 0);
  first.send(output(3, "def"), 2);
  first.send(output(0, "abc"), 1);
  assert.deepEqual(screen.shown, ["abc", "def"]);

  // Message 3 never comes: 4 waits for it, and then the stream is lost.
  first.send(output(9, "jkl"), 4);
  nextAttach();
  t.mock.timers.tick(LOST_MS);
  await settle();
  assert.deepEqual(screen.shown, ["abc", "def"]);
  const again = calls.findLast(c => c.cmd === "terminal_attach")!;
  assert.equal(count("terminal_attach"), 2);
  assert.equal((again.args as { from: number }).from, 6);
});

test("a gap in a disposed terminal stream cannot attach it again", async t => {
  t.mock.timers.enable({ apis: ["setTimeout"] });
  calls.length = 0;
  const term = paneTerm(ROW, fakeScreen(), { onEnd() {}, onLink() {}, onEscape() {} });
  const stream = nextAttach();
  attach(term);
  stream.send({ kind: "attached" }, 0);
  stream.send(output(3, "def"), 2);

  dropTerm(term);
  t.mock.timers.tick(LOST_MS + RETRY_MS);
  stream.send(output(0, "abc"), 1);
  stream.end();
  await settle();

  assert.equal(term.link.kind, "disposed");
  assert.equal(count("terminal_attach"), 1);
});

test("output replayed on attach is told apart from live output, except for a terminal just opened", t => {
  t.mock.timers.enable({ apis: ["Date"] });
  const now = { at: 1000 };
  t.mock.method(performance, "now", () => now.at);
  calls.length = 0;

  // A terminal that already ran: what comes at once is the node's replay,
  // until a pause in the output.
  const screen = fakeScreen();
  const term = paneTerm(ROW, screen, { onEnd() {}, onLink() {}, onEscape() {} });
  const stream = nextAttach();
  attach(term);
  stream.send({ kind: "attached" });
  stream.send(output(0, "old\x1b[c"));
  now.at += 50;
  stream.send(output(6, "older"));
  now.at += 1000;
  stream.send(output(11, "new"));
  assert.deepEqual(screen.replayed, ["old\x1b[c", "older"]);
  assert.deepEqual(screen.shown, ["old\x1b[c", "older", "new"]);

  // One this pane just started has nothing to replay.
  const fresh = fakeScreen();
  const opened = paneTerm(ROW, fresh, { onEnd() {}, onLink() {}, onEscape() {} }, true);
  const first = nextAttach();
  attach(opened);
  first.send({ kind: "attached" });
  first.send(output(0, "\x1b[c"));
  assert.deepEqual(fresh.replayed, []);
  assert.deepEqual(fresh.shown, ["\x1b[c"]);
});
