import test from "node:test";
import assert from "node:assert/strict";
import { opensNewWindow, startView, windowTitle } from "../src/projectwindow.ts";

test("cmd-click and middle-click open a project in a new window", () => {
  assert.equal(opensNewWindow({ button: 0, metaKey: true }), true);
  assert.equal(opensNewWindow({ button: 1, metaKey: false }), true);
  assert.equal(opensNewWindow({ button: 1, metaKey: true }), true);
});

test("a plain click or a right-click stays in the window", () => {
  assert.equal(opensNewWindow({ button: 0, metaKey: false }), false);
  assert.equal(opensNewWindow({ button: 2, metaKey: false }), false);
  assert.equal(opensNewWindow({ button: 2, metaKey: true }), false);
});

test("a project's window starts on that project", () => {
  assert.deepEqual(startView("org/site"), { view: "projects", arg: { project: "org/site" } });
});

test("the main window starts on what was left for it", () => {
  assert.equal(startView(null), null);
  assert.equal(startView(""), null);
});

test("a project's window is titled with its name", () => {
  assert.equal(windowTitle("org/site"), "org/site");
  assert.equal(windowTitle(null), "Beep");
});
