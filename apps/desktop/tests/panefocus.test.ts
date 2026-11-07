import { bar, runFrames } from "./dom-env.ts";
import test from "node:test";
import assert from "node:assert/strict";
import { P, drawPane, open, paneFrame, paneOf, type MediaTab } from "../src/agentpane/media.ts";
import { paneTerm } from "../src/agentpane/terminal.ts";
import type { Screen, TerminalRow } from "../src/terminals/terminals.ts";

test("pane refreshes keep conversation focus and terminal selection still focuses the shell", t => {
  t.mock.timers.enable({ apis: ["setInterval"] });
  bar.innerHTML = `<div id="agshell">${paneFrame('<textarea id="composer"></textarea>')}</div>`;
  const composer = document.getElementById("composer")!;
  const shell = document.getElementById("agshell")!;
  Object.defineProperty(shell, "clientWidth", { value: 1200 });
  const active = (el: Element) => Object.defineProperty(document, "activeElement", { configurable: true, value: el });
  const row: TerminalRow = {
    id: "t1", project: "", machine_id: "m1", machine_name: "build-box", name: "shell",
    command: "", cwd: "/srv/project", state: "running", code: null, started_at: 1,
  };
  let fits = 0;
  const screen: Screen = {
    el: document.createElement("div"),
    write() {}, note() {}, size: () => ({ cols: 100, rows: 30 }),
    fit: () => { fits++; }, focus: () => active(screen.el),
    focused: () => screen.el.contains(document.activeElement),
    onData() {}, onResize() {}, onEscape() {}, setInput() {}, dispose() {},
  };
  const tab: MediaTab = {
    id: "term:1", kind: "term", name: "shell", path: null, url: null,
    size: 0, done: 0, error: "", el: null,
    term: paneTerm(row, screen, { onEnd() {}, onLink() {}, onEscape() {} }),
  };
  P.tabs = [tab];
  P.on.right = tab.id;
  active(composer);
  for (let i = 0; i < 3; i++) {
    drawPane();
    runFrames();
    assert.ok(document.activeElement === composer, "the composer keeps focus");
  }
  assert.equal(fits, 3, "the terminal still fits on refresh");

  // Opening an existing tab gives its terminal the keys.
  const pane = paneOf("right")!;
  t.mock.method(pane, "focus", () => active(pane));
  open(tab);
  runFrames();
  assert.ok(document.activeElement === screen.el, "the terminal takes focus when selected");

  // A click back into the composer wins over a pending terminal frame.
  drawPane();
  active(composer);
  runFrames();
  assert.ok(document.activeElement === composer, "the composer keeps focus");
  P.tabs = [];
});
