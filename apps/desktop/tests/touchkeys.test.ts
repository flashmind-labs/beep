import { test } from "node:test";
import assert from "node:assert/strict";
import { keyBytes, TermKeyState } from "../src/stores/terminals/touchkeys-state.ts";

const none = new Set<never>();

test("arrows carry the modifier number", () => {
  assert.equal(keyBytes("up", new Set(["shift"])), "\x1b[1;2A");
  assert.equal(keyBytes("up", new Set(["ctrl"])), "\x1b[1;5A");
  assert.equal(keyBytes("left", new Set(["alt"])), "\x1b[1;3D");
  assert.equal(keyBytes("right", new Set(["ctrl", "alt", "shift"])), "\x1b[1;8C");
  assert.equal(keyBytes("down", none), "\x1b[B");
});

test("alt x is esc x", () => {
  assert.equal(keyBytes({ char: "x" }, new Set(["alt"])), "\x1bx");
});

test("sticky ctrl clears after one key", () => {
  const keys = new TermKeyState();
  keys.toggle("ctrl");
  assert.equal(keys.press({ char: "c" }), "\x03");
  assert.equal(keys.press({ char: "c" }), "c");
});

test("home, end, esc, tab, shift+tab", () => {
  assert.equal(keyBytes("home", none), "\x1b[H");
  assert.equal(keyBytes("end", none), "\x1b[F");
  assert.equal(keyBytes("esc", none), "\x1b");
  assert.equal(keyBytes("tab", none), "\t");
  assert.equal(keyBytes("tab", new Set(["shift"])), "\x1b[Z");
});

test("emulator replies and pastes pass unchanged and stay armed", () => {
  const keys = new TermKeyState();
  keys.toggle("ctrl");
  assert.equal(keys.applyTyped("\x1b[?62;c"), "\x1b[?62;c");
  assert.equal(keys.applyTyped("ls"), "ls");
  assert.equal(keys.applyTyped("\r"), "\r");
  assert.equal(keys.applyTyped("c"), "\x03");
});

test("toggling twice disarms", () => {
  const keys = new TermKeyState();
  keys.toggle("alt");
  keys.toggle("alt");
  assert.equal(keys.press({ char: "x" }), "x");
});
