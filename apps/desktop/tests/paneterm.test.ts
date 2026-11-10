import { answers, calls, nativeEvent } from "./bar-env.ts";
import test from "node:test";
import assert from "node:assert/strict";
import { attach, dropTerm, paneTerm, termStatus } from "../src/agentpane/terminal.ts";
import { P, closeTab, closePaneTabs, type MediaTab } from "../src/agentpane/media.ts";
import { makeTerminals, ALL, type Screen, type TermEvent, type TerminalRow } from "../src/terminals/terminals.ts";

const ROW: TerminalRow = {
  id: "t1", project: "", machine_id: "m1", machine_name: "build-box", name: "shell",
  command: "", cwd: "/srv/project", state: "running", code: null, started_at: 1,
};

/// A screen that keeps what reaches it, at a size the test sets.
function fakeScreen(cols = 100, rows = 30) {
  const shown: string[] = [];
  let resized: (size: { cols: number; rows: number }) => void = () => {};
  let escaped: () => void = () => {};
  const screen: Screen & { shown: string[]; resize(cols: number, rows: number): void; pressEsc(): void } = {
    el: {} as HTMLElement,
    shown,
    write: data => shown.push(new TextDecoder().decode(data)),
    note: text => shown.push(text),
    size: () => ({ cols, rows }),
    fit() {},
    focus() {},
    focused: () => false,
    onData() {},
    onResize: cb => { resized = cb; },
    onEscape: cb => { escaped = cb; },
    pressEsc: () => escaped(),
    setInput() {},
    dispose() {},
    resize(c, r) { cols = c; rows = r; resized({ cols, rows }); },
  };
  return screen;
}

/// The attach the next `terminal_attach` starts: its event channel, and
/// how to end its stream.
function nextAttach() {
  let end!: () => void;
  answers.terminal_attach = new Promise<null>(resolve => { end = () => resolve(null); });
  return {
    end,
    send(ev: TermEvent) {
      const call = calls.findLast(c => c.cmd === "terminal_attach")!;
      (call.args as { onEvent: { onmessage(ev: TermEvent): void } }).onEvent.onmessage(ev);
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
  first.send({ kind: "output", at: 0, data: btoa("% ") });
  const second = nextAttach();
  first.end();
  await settle();
  assert.equal(termStatus(term, "build-box"), "Reconnecting to build-box…");
  assert.deepEqual(screen.shown, ["% "], "nothing of ours was written to the screen");
  t.mock.timers.tick(1000);
  assert.equal(count("terminal_attach"), 2);
  assert.equal((calls.at(-1)!.args as { from: number }).from, 2, "it picks up after what the screen shows");
  // The node resends from where the screen got to; nothing shows twice.
  second.send({ kind: "attached" });
  second.send({ kind: "output", at: 0, data: btoa("% ls\r\n") });
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
    assert.equal(term.link, "attached");
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
    assert.equal(term.link, "ended");
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
  assert.equal(term.link, "attached");
  assert.equal(term.tries, 0);
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
    Channel: class { onmessage = (_: unknown) => {}; } as never,
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
  t.mock.timers.tick(1000);
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
  const tab: MediaTab = { id: "term:shell 1", kind: "term", name: "shell 1", path: null, url: null, size: 0, done: 0, error: "", rev: 0, viewHidden: false, note: null, video: null, videoAt: 0, term };
  P.tabs.push(tab);
  closeTab(tab.id);
  await settle();
  assert.deepEqual(calls.filter(c => c.cmd === "terminal_close").map(c => c.args), [{ id: "t1" }]);
  assert.ok(!P.tabs.includes(tab), "the tab went");
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
  const tab: MediaTab = { id: "term:shell 2", kind: "term", name: "shell 2", path: null, url: null, size: 0, done: 0, error: "", rev: 0, viewHidden: false, note: null, video: null, videoAt: 0, term };
  P.tabs.push(tab);
  answers.terminal_close = Promise.reject(new Error("build-box: the node stopped answering"));
  nextAttach();
  closeTab(tab.id);
  await settle();
  delete answers.terminal_close;
  assert.ok(P.tabs.includes(tab), "the tab stays");
  assert.equal(term.link, "attaching");
  assert.equal(count("terminal_attach"), 2);
  P.tabs.length = 0;
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
  P.tabs.push({ id: "term:shell", kind: "term", name: "shell", path: null, url: null, size: 0, done: 0, error: "", rev: 0, viewHidden: false, note: null, video: null, videoAt: 0, term });
  closePaneTabs();
  await settle();
  assert.equal(P.tabs.length, 0);
  assert.equal(disposed, 1);
  assert.equal(count("terminal_close"), 0);
  stream.end();
  await settle();
  assert.equal(count("terminal_attach"), 1);
});

test("leaving keeps starting and existing terminals alive", async t => {
  noPane(t);
  calls.length = 0;
  const starting: MediaTab = { id: "term:new", kind: "term", name: "new", path: null, url: null, size: 0, done: 0, error: "", rev: 0, viewHidden: false, note: null, video: null, videoAt: 0 };
  P.tabs.push(starting, { ...starting, id: "term:existing", existing: ROW });
  closePaneTabs();
  await settle();
  assert.equal(starting.closed, undefined);
  assert.equal(P.tabs.length, 0);
  assert.equal(count("terminal_close"), 0);
});

test("leaving during a tab close does not send a second stop", async t => {
  noPane(t);
  calls.length = 0;
  let stopped!: () => void;
  answers.terminal_close = new Promise<void>(resolve => { stopped = resolve; });
  const term = paneTerm(ROW, fakeScreen(), { onEnd() {}, onLink() {}, onEscape() {} });
  P.tabs.push({ id: "term:shell", kind: "term", name: "shell", path: null, url: null, size: 0, done: 0, error: "", rev: 0, viewHidden: false, note: null, video: null, videoAt: 0, term });
  closeTab("term:shell");
  closePaneTabs();
  stopped();
  await settle();
  delete answers.terminal_close;
  assert.equal(count("terminal_close"), 1);
  assert.equal(P.tabs.length, 0);
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
  P.tabs.push({ id: "term:shell", kind: "term", name: "shell", path: null, url: null, size: 0, done: 0, error: "", rev: 0, viewHidden: false, note: null, video: null, videoAt: 0, term });
  closeTab("term:shell");
  closePaneTabs();
  refuse(new Error("the node stopped answering"));
  await settle();
  delete answers.terminal_close;
  assert.equal(term.link, "detached");
  assert.equal(count("terminal_attach"), 1);
  assert.equal(disposed, 1);
  assert.equal(P.tabs.length, 0);
});

/// Sends `ev` down the latest stream of view `view`.
function sendTo(view: string, ev: TermEvent) {
  const call = calls.findLast(c => c.cmd === "terminal_attach" && (c.args as { view: string }).view === view)!;
  (call.args as { onEvent: { onmessage(ev: TermEvent): void } }).onEvent.onmessage(ev);
}

test("a terminal hidden then shown again shows the output from meanwhile, in order", () => {
  nativeEvent("bar-shown");
  calls.length = 0;
  nextAttach();
  const screen = fakeScreen();
  const term = paneTerm(ROW, screen, { onEnd() {}, onLink() {}, onEscape() {} });
  const mine = (cmd: string) => calls.filter(c => c.cmd === cmd && (c.args as { view: string }).view === term.view);
  attach(term);
  sendTo(term.view, { kind: "attached" });
  sendTo(term.view, { kind: "output", at: 0, data: btoa("one\r\n") });

  nativeEvent("bar-hidden");
  assert.equal(mine("terminal_detach").length, 1, "a hidden pane lets go of the stream");
  sendTo(term.view, { kind: "output", at: 5, data: btoa("two\r\n") });
  assert.deepEqual(screen.shown, ["one\r\n"], "nothing is drawn while hidden");

  nativeEvent("bar-shown");
  assert.equal(mine("terminal_attach").length, 2);
  assert.equal((mine("terminal_attach").at(-1)!.args as { from: number }).from, 5, "it picks up after what the screen shows");
  sendTo(term.view, { kind: "attached" });
  sendTo(term.view, { kind: "output", at: 0, data: btoa("one\r\ntwo\r\nthree\r\n") });
  assert.deepEqual(screen.shown, ["one\r\n", "two\r\nthree\r\n"]);
  assert.equal(term.link, "attached");
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
    Channel: class { onmessage = (_: unknown) => {}; } as never,
    makeScreen: () => screen,
    ago: () => "",
    redraw() {},
  });
  await terms.load(ALL);
  terms.show("t1");
  const view = (calls.findLast(c => c.cmd === "terminal_attach")!.args as { view: string }).view;
  sendTo(view, { kind: "attached" });
  sendTo(view, { kind: "output", at: 0, data: btoa("a") });

  nativeEvent("center-hidden");
  const mine = (cmd: string) => calls.filter(c => c.cmd === cmd && (c.args as { view: string }).view === view);
  assert.equal(mine("terminal_detach").length, 1);
  nativeEvent("center-shown");
  assert.equal((mine("terminal_attach").at(-1)!.args as { from: number }).from, 1);
  sendTo(view, { kind: "output", at: 0, data: btoa("ab") });
  assert.deepEqual(screen.shown, ["a", "b"]);
  terms.stop();
});
