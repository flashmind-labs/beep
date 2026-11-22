import test from "node:test";
import assert from "node:assert/strict";
import { windowCommand, windowTakes, type WindowKeyContext } from "../src/ui/windowkeys.ts";

const at: WindowKeyContext = { page: "home", project: null, alone: false };
const key = (key: string, extra: Partial<KeyboardEvent> = {}) => ({ key, ctrlKey: false, metaKey: true, shiftKey: false, altKey: false, defaultPrevented: false, ...extra });

test("the window owns search, Settings, the shortcuts sheet, the sidebar and the Assistant", () => {
  for (const owned of ["k", "K", ",", "/", "e", "l"]) assert.equal(windowTakes(owned), true);
  for (const pane of ["m", "r", "n"]) assert.equal(windowTakes(pane), false);
});

test("window shortcuts resolve to domain commands", () => {
  assert.deepEqual(windowCommand(key("k"), at, false), { kind: "palette" });
  assert.deepEqual(windowCommand(key(","), at, false), { kind: "settings" });
  assert.deepEqual(windowCommand(key("/"), at, false), { kind: "shortcuts" });
  assert.deepEqual(windowCommand(key("e"), at, false), { kind: "sidebar" });
  assert.deepEqual(windowCommand(key("l"), at, false), { kind: "assistant" });
  assert.equal(windowCommand(key("l"), { ...at, alone: true }, false), null);
  assert.deepEqual(windowCommand(key("n"), at, false), { kind: "new-agent" });
  assert.equal(windowCommand(key("q"), at, false), null);
});

test("window history and closing retain priority and modifier behavior", () => {
  assert.deepEqual(windowCommand(key("[", { altKey: true }), at, false), { kind: "history", forward: false });
  assert.deepEqual(windowCommand(key("]"), at, false), { kind: "history", forward: true });
  assert.equal(windowCommand(key("[", { shiftKey: true }), at, false), null);
  assert.deepEqual(windowCommand(key("w", { altKey: true }), at, false), { kind: "close" });
  assert.equal(windowCommand(key("w", { shiftKey: true }), at, false), null);
});

test("pane-specific commands stay with the pane", () => {
  assert.equal(windowCommand(key("m"), { ...at, page: "pane" }, false), null);
  assert.equal(windowCommand(key("n"), { ...at, page: "pane" }, false), null);
});

test("project window command requires the main window's selected project", () => {
  const project: WindowKeyContext = { ...at, page: "projects", project: "acme/widget" };
  assert.deepEqual(windowCommand(key("n", { shiftKey: true }), project, false), { kind: "project-window", project: "acme/widget" });
  assert.deepEqual(windowCommand(key("n", { shiftKey: true }), { ...project, alone: true }, false), { kind: "new-agent" });
  assert.deepEqual(windowCommand(key("n", { shiftKey: true }), { ...project, project: null }, false), { kind: "new-agent" });
});

test("terminal creation requires an unhandled shortcut with no extra modifiers", () => {
  assert.deepEqual(windowCommand(key("t"), at, false), { kind: "project-terminal" });

  for (const extra of [{ shiftKey: true }, { altKey: true }, { ctrlKey: true }, { defaultPrevented: true }]) {
    assert.equal(windowCommand(key("t", extra), at, false), null);
  }

  assert.deepEqual(windowCommand(key("t", { metaKey: false, ctrlKey: true }), at, true), { kind: "project-terminal" });
});

test("app modifier and permissive matching remain platform-specific", () => {
  assert.equal(windowCommand(key("k", { metaKey: false, ctrlKey: true }), at, false), null);
  assert.equal(windowCommand(key("k"), at, true), null);
  assert.deepEqual(windowCommand(key("k", { metaKey: false, ctrlKey: true }), at, true), { kind: "palette" });
  assert.deepEqual(windowCommand(key("K", { altKey: true, shiftKey: true }), at, false), { kind: "palette" });
  assert.deepEqual(windowCommand(key("k", { defaultPrevented: true }), at, false), { kind: "palette" });
});
