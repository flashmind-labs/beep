import test from "node:test";
import assert from "node:assert/strict";
import { wordKeyBytes } from "../src/terminals/keys-state.ts";

const k = (key: string, m: Partial<Record<"metaKey" | "ctrlKey" | "altKey" | "shiftKey", boolean>> = {}) =>
  wordKeyBytes({ key, metaKey: false, ctrlKey: false, altKey: false, shiftKey: false, ...m });

test("word and line keys", () => {
  assert.equal(k("ArrowLeft", { ctrlKey: true }), "\x1bb");
  assert.equal(k("ArrowRight", { altKey: true }), "\x1bf");
  assert.equal(k("ArrowLeft", { metaKey: true }), "\x01");
  assert.equal(k("ArrowRight", { metaKey: true }), "\x05");
  assert.equal(k("Backspace", { altKey: true }), "\x1b\x7f");
  assert.equal(k("Backspace", { metaKey: true }), "\x15");
  assert.equal(k("Delete", { altKey: true }), "\x1bd");
});

test("other keys are left to xterm", () => {
  assert.equal(k("ArrowLeft"), null);
  assert.equal(k("Backspace"), null);
  assert.equal(k("c", { metaKey: true }), null);
  assert.equal(k("ArrowLeft", { ctrlKey: true, shiftKey: true }), null);
});
