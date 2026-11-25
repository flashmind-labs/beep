import test from "node:test";
import assert from "node:assert/strict";
import { loadFull } from "../src/review/diff-load.ts";

test("a failed read of a left-out file leaves it folded and says why", async () => {
  const file = { path: "big.txt", old_path: null, status: "modified", additions: 0, deletions: 0, left_out: "too large", hunks: [] };
  const state: Record<string, any> = {
    view: { files: [file] }, pending: new Set<string>(), folded: new Map([["big.txt", true]]), uncommitted: false,
    list: [], editors: new Map(), orphaned: new Set(), error: "",
  };
  const v: any = {
    state, seq: 1, set: (change: object) => Object.assign(state, change),
    src: { load: async () => { throw new Error("read refused"); } },
    selectionHeld: new Set(), sections: new Map(), store: { setState() {} },
  };

  await loadFull(v, file as never);
  assert.match(state.error, /read refused/);
  assert.equal(state.folded.get("big.txt"), true);
});
