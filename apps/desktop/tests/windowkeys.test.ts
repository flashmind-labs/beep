import test from "node:test";
import assert from "node:assert/strict";
import { windowCommand, windowTakes, type WindowKeyContext } from "../src/ui/windowkeys.ts";

const at: WindowKeyContext = { page: "home", pane: null, project: null, projectWindow: false, subscribed: true };
const key = (key: string, extra: Partial<KeyboardEvent> = {}) => ({ key, ctrlKey: false, metaKey: true, shiftKey: false, altKey: false, defaultPrevented: false, ...extra });

test("the window owns research except while renaming a chat", () => {
  assert.equal(windowTakes("r", "chat"), true);
  assert.equal(windowTakes("R", "agent"), true);
  assert.equal(windowTakes("r", null), true);
  assert.equal(windowTakes("r"), true);
  assert.equal(windowTakes("r", "chats"), false);
  assert.equal(windowTakes("m", "agentnew"), false);
});

test("window shortcuts resolve to domain commands", () => {
  assert.deepEqual(windowCommand(key("k"), at, false), { kind: "palette" });
  assert.deepEqual(windowCommand(key(","), at, false), { kind: "settings" });
  assert.deepEqual(windowCommand(key("/"), at, false), { kind: "shortcuts" });
  assert.deepEqual(windowCommand(key("m"), at, false), { kind: "pane", view: "memories" });
  assert.deepEqual(windowCommand(key("j"), at, false), { kind: "pane", view: "routines" });
  assert.deepEqual(windowCommand(key("l"), at, false), { kind: "pane", view: "skills" });
  assert.deepEqual(windowCommand(key("r"), at, false), { kind: "research" });
  assert.deepEqual(windowCommand(key("n"), at, false), { kind: "new-agent" });
  assert.equal(windowCommand(key("q"), at, false), null);
});

test("window history and closing retain priority and modifier behavior", () => {
  const unsubscribed = { ...at, subscribed: false };
  assert.deepEqual(windowCommand(key("[", { altKey: true }), unsubscribed, false), { kind: "history", forward: false });
  assert.deepEqual(windowCommand(key("]"), unsubscribed, false), { kind: "history", forward: true });
  assert.equal(windowCommand(key("[", { shiftKey: true }), at, false), null);
  assert.deepEqual(windowCommand(key("w", { altKey: true }), at, false), { kind: "close" });
  assert.equal(windowCommand(key("w", { shiftKey: true }), at, false), null);
});

test("subscription filtering leaves the palette available", () => {
  const unsubscribed = { ...at, subscribed: false };
  assert.deepEqual(windowCommand(key("k"), unsubscribed, false), { kind: "palette" });
  for (const command of ["m", "j", "l", "r", "g"]) {
    assert.deepEqual(windowCommand(key(command), unsubscribed, false), { kind: "blocked" });
  }
});

test("pane-specific commands stay with the pane", () => {
  assert.equal(windowCommand(key("m"), { ...at, page: "pane", pane: "agentnew" }, false), null);
  assert.equal(windowCommand(key("r"), { ...at, page: "pane", pane: "chats" }, false), null);
  assert.equal(windowCommand(key("n"), { ...at, page: "pane", pane: "agent" }, false), null);
  assert.deepEqual(windowCommand(key("r"), { ...at, page: "pane", pane: "agent" }, false), { kind: "research" });
});

test("project window command requires the main window's selected project", () => {
  const project: WindowKeyContext = { ...at, page: "projects", project: "acme/widget" };
  assert.deepEqual(windowCommand(key("n", { shiftKey: true }), project, false), { kind: "project-window", project: "acme/widget" });
  assert.deepEqual(windowCommand(key("n", { shiftKey: true }), { ...project, projectWindow: true }, false), { kind: "new-agent" });
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
