import { answers, calls } from "./bar-env.ts";
import test from "node:test";
import assert from "node:assert/strict";
import { attach, paneTerm, termStatus } from "../src/bar/paneterm.ts";
import { makeTerminals, ALL, type Screen, type TermEvent, type TerminalRow } from "../src/terminals.ts";

const ROW: TerminalRow = {
  id: "t1", project: "", machine_id: "m1", machine_name: "build-box", name: "shell",
  command: "", cwd: "/srv/project", state: "running", code: null, started_at: 1,
};

/// A screen that keeps what reaches it, at a size the test sets.
function fakeScreen(cols = 100, rows = 30) {
  const shown: string[] = [];
  let resized: (size: { cols: number; rows: number }) => void = () => {};
  const screen: Screen & { shown: string[]; resize(cols: number, rows: number): void } = {
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
  const term = paneTerm(ROW, screen, { onEnd() {}, onLink() {} });
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

test("a stream that a newer attach replaced ending does not reconnect", async t => {
  t.mock.timers.enable({ apis: ["setTimeout"] });
  calls.length = 0;
  const term = paneTerm(ROW, fakeScreen(), { onEnd() {}, onLink() {} });
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
    h: (() => ({})) as never,
    button: (() => ({})) as never,
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
