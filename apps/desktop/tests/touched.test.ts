import test from "node:test";
import assert from "node:assert/strict";
import { changedIn, touches, touchesRepo } from "../src/agentpane/touched.ts";

test("the changed paths come from the views native code sends or from the node's listing", () => {
  assert.deepEqual(changedIn({ views: [], changed: ["a.txt"] }), ["a.txt"]);
  assert.deepEqual(changedIn({ Sessions: { Tree: { tree: {}, changed: ["src/a.rs"] } } }), ["src/a.rs"]);
  assert.deepEqual(changedIn({ Sessions: { Tree: { tree: {} } } }), []);
  assert.deepEqual(changedIn(undefined), []);
});

test("only files and Git's HEAD, index and refs move a checkout's diff and history", () => {
  assert.equal(touchesRepo([]), true, "no paths means anything may have changed");
  assert.equal(touchesRepo(["src/a.rs"]), true);
  assert.equal(touchesRepo([".git"]), true);
  assert.equal(touchesRepo([".git/HEAD"]), true);
  assert.equal(touchesRepo([".git/refs/heads/main"]), true);
  assert.equal(touchesRepo([".git/FETCH_HEAD", ".git/config"]), false);
  assert.equal(touchesRepo([".git/config", "a.txt"]), true);
});

test("a change touches a file, the folders holding it and what is under it", () => {
  assert.equal(touches(["site/index.html"], "site/index.html"), true);
  assert.equal(touches(["site"], "site/index.html"), true);
  assert.equal(touches(["site/img"], "site"), true);
  assert.equal(touches(["site/other.html"], "site/index.html"), false);
  assert.equal(touches(["site2/index.html"], "site/index.html"), false);
  assert.equal(touches([], "site/index.html"), true);
});
