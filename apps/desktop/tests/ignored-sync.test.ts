import test from "node:test";
import assert from "node:assert/strict";
import { ignoredSelection } from "../src/project/ignored-sync-state.ts";

test("ignored sync requires an explicit folder and keeps the selection when disabled", () => {
  assert.deepEqual(ignoredSelection(false, ""), { enabled:false, folders:[] });
  assert.throws(() => ignoredSelection(true, ""), /at least one/);
  assert.deepEqual(ignoredSelection(false, "models/\n models\nassets/captures"), { enabled:false, folders:["assets/captures", "models"] });
});

test("ignored folder paths cannot escape, overlap, or select Git and build state", () => {
  for (const paths of ["../outside", "/models", "models/../../src", ".GIT/objects", "C:\\data", "models\\nested", "models\nmodels/nested", "Models\nmodels", "target", "node_modules", ".worktrees", "assets//models"]) {
    assert.throws(() => ignoredSelection(true, paths), paths);
  }
});
