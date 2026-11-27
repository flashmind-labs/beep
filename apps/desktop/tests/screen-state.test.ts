import { test } from "node:test";
import assert from "node:assert/strict";
import { cellAt, fitGrid, routeKey, wheelAction, withSymbols } from "../src/stores/terminals/screen-state.ts";

const CELL = { width: 16, height: 32 };

function key(k: string, mods: Partial<KeyboardEvent> = {}) {
  return { key: k, code: "", metaKey: false, ctrlKey: false, altKey: false, shiftKey: false, isComposing: false, ...mods };
}

test("a grid fits whole cells at the screen's pixel ratio", () => {
  assert.deepEqual(fitGrid(800, 400, 2, CELL), { cols: 100, rows: 25 });
  assert.deepEqual(fitGrid(805, 415, 2, CELL), { cols: 100, rows: 25 });
  assert.deepEqual(fitGrid(1, 1, 1, CELL), { cols: 2, rows: 1 });
});

test("a pointer maps to the cell under it, inside the grid", () => {
  const grid = { cols: 10, rows: 5 };
  assert.deepEqual(cellAt(17, 33, 2, CELL, grid), { col: 2, row: 2, x: 34, y: 66 });
  assert.deepEqual(cellAt(-5, 9999, 2, CELL, grid), { col: 0, row: 4, x: 0, y: 19998 });
});

test("keys go to the terminal, the clipboard, the app or out", () => {
  assert.deepEqual(routeKey(key("a"), true, false, false), { kind: "terminal" });
  assert.deepEqual(routeKey(key("c", { metaKey: true }), true, false, false), { kind: "copy" });
  assert.deepEqual(routeKey(key("v", { metaKey: true }), true, false, false), { kind: "paste" });
  assert.deepEqual(routeKey(key("w", { metaKey: true }), true, false, false), { kind: "app" });
  // Off macOS, Ctrl+C is the terminal's; Ctrl+Shift+C copies.
  assert.deepEqual(routeKey(key("c", { ctrlKey: true }), false, false, false), { kind: "terminal" });
  assert.deepEqual(routeKey(key("C", { ctrlKey: true, shiftKey: true }), false, false, false), { kind: "copy" });
  assert.deepEqual(routeKey(key("ArrowLeft", { metaKey: true }), true, false, false), { kind: "word", bytes: "\x01" });
  assert.deepEqual(routeKey(key("Escape"), true, false, true), { kind: "leave" });
  // vim on the alternate screen keeps Esc.
  assert.deepEqual(routeKey(key("Escape"), true, true, true), { kind: "terminal" });
  assert.deepEqual(routeKey(key("Escape"), true, false, false), { kind: "terminal" });
});

test("the wheel scrolls, sends arrows or mouse buttons", () => {
  assert.deepEqual(wheelAction(-100, false, false, false), { kind: "scroll", rows: -3 });
  assert.deepEqual(wheelAction(3, true, false, false), { kind: "scroll", rows: 3 });
  assert.deepEqual(wheelAction(100, false, false, true), { kind: "keys", code: "ArrowDown", times: 3 });
  assert.deepEqual(wheelAction(-100, false, true, true), { kind: "mouse", button: "wheel-up", times: 1 });
  assert.deepEqual(wheelAction(-4, false, false, false), { kind: "scroll", rows: -1 });
  assert.equal(wheelAction(0, false, false, false), null);
});

test("the symbols font goes before the generic fallbacks", () => {
  assert.equal(withSymbols(`"IBM Plex Mono", ui-monospace, Menlo, monospace`), `"IBM Plex Mono", 'Symbols Nerd Font Mono', ui-monospace, Menlo, monospace`);
  assert.equal(withSymbols(`"Geist Mono", "IBM Plex Mono"`), `"Geist Mono", "IBM Plex Mono", 'Symbols Nerd Font Mono'`);
});
