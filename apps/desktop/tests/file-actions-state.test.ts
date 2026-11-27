import { test } from "node:test";
import assert from "node:assert/strict";
import { selectFile, selectionRoots, fileNameError, canPasteFiles, type FileSelection } from "../src/stores/agentpane/file-actions-state.ts";
const paths = ["src", "src/a", "src/b", "other"];
const empty = (): FileSelection => ({ picked: null, anchor: null, selected: new Set() });
test("selection toggles and extends from a stable anchor in visible order", () => {
  let s = selectFile(empty(), paths, "src/a");
  s = selectFile(s, paths, "other", true);
  assert.deepEqual([...s.selected], ["src/a", "other"]);
  s = selectFile(s, paths, "other", true);
  assert.deepEqual([...s.selected], ["src/a"]);
  s = selectFile(selectFile(empty(), paths, "src/a"), paths, "other", false, true);
  assert.deepEqual([...s.selected], ["src/a", "src/b", "other"]);
  s = selectFile(s, paths, "src/b", false, true);
  assert.deepEqual([...s.selected], ["src/a", "src/b"]);
});
test("bulk actions cover nested selections only once", () => {
  assert.deepEqual(selectionRoots(["src/a", "src", "src", "src2", "other"]), ["src", "src2", "other"]);
});
test("paste stays on its machine and root and rejects descendants", () => {
  const clip = { key: "machine\n/work", paths: ["src"], cut: true };
  assert.ok(canPasteFiles(clip, clip.key, "other"));
  assert.ok(!canPasteFiles(clip, "different", "other"));
  assert.ok(!canPasteFiles(clip, clip.key, "src"));
  assert.ok(!canPasteFiles(clip, clip.key, "src/nested"));
});
test("names cannot escape their parent or edit Git metadata", () => {
  for (const bad of ["", ".", "..", ".git", ".GIT", "a/b", "a\\b", "C:a", "a\0b"]) assert.ok(fileNameError(bad), bad);
  for (const good of ["notes.md", "two words", "quote's.txt"]) assert.equal(fileNameError(good), "");
});
