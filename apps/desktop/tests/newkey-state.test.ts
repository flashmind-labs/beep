import test from "node:test";
import assert from "node:assert/strict";
import { isNewKey, newTermLike } from "../src/terminals/newkey-state.ts";

const term = (machine_id: string, cwd: string, focused = false) => ({ machine_id, machine_name: machine_id, cwd, project: "", focused });

test("⌘N copies the focused terminal, else the first on screen, else opens an agent", () => {
  assert.equal(newTermLike([]), null);
  const a = term("m1", "/srv/a"), b = term("m2", "/srv/b", true);
  assert.equal(newTermLike([a]), a);
  assert.equal(newTermLike([a, b]), b);
  // A terminal tab with no machine to run on is not one to copy.
  assert.equal(newTermLike([term("", "/srv/c", true), a]), a);
  assert.equal(newTermLike([term("", "/srv/c")]), null);
});

test("only plain ⌘N is the new key", () => {
  const k = (key: string, mods: Partial<KeyboardEvent> = {}) => ({ key, metaKey: true, shiftKey: false, altKey: false, ctrlKey: false, ...mods });
  assert.equal(isNewKey(k("n")), true);
  assert.equal(isNewKey(k("N")), true);
  assert.equal(isNewKey(k("n", { shiftKey: true })), false);
  assert.equal(isNewKey(k("n", { metaKey: false })), false);
  assert.equal(isNewKey(k("t")), false);
});
