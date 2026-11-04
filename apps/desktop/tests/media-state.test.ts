import test from "node:test";
import assert from "node:assert/strict";
import { DEFAULT_LAYOUT, KEPT_TABS, SIDE_BY_SIDE_MIN, ZOOM_MAX, baseName, moveTab, readLayout, resolvePath, splitTabs, tabDock, zoomAt } from "../src/agentpane/media-state.ts";

test("a relative path is the session folder's", () => {
  assert.equal(resolvePath("/srv/project", "out/a.glb"), "/srv/project/out/a.glb");
  assert.equal(resolvePath("/srv/project/", "./out/./a.glb"), "/srv/project/out/a.glb");
  assert.equal(resolvePath("/srv/project/app", "../shared/a.pdf"), "/srv/project/shared/a.pdf");
  assert.equal(resolvePath("/srv/project", "/tmp/a.mp4"), "/tmp/a.mp4");
  assert.equal(resolvePath("/srv/project", "~/renders/a.mp4"), "~/renders/a.mp4");
  assert.equal(resolvePath("/srv", "../../../a.pdf"), "/a.pdf");
  assert.equal(baseName("/srv/project/out/a.glb"), "a.glb");
});

test("a stored layout that does not read is the default", () => {
  assert.deepEqual(readLayout(null), DEFAULT_LAYOUT);
  assert.deepEqual(readLayout("not json"), DEFAULT_LAYOUT);
  assert.deepEqual(readLayout("[1]"), { ...DEFAULT_LAYOUT });
  // One from before tabs had docks: folded is both docks folded.
  assert.deepEqual(readLayout('{"dock":"bottom","right":412.4,"bottom":-3,"hidden":true}'),
    { dock: "bottom", right: 412, bottom: null, hidden: { right: true, bottom: true }, tabs: {} });
  assert.deepEqual(readLayout('{"dock":"left","right":"wide"}'), DEFAULT_LAYOUT);
  assert.deepEqual(readLayout('{"hidden":{"bottom":true},"tabs":{"files":"right","term:a":"bottom","x":"left","data:image/png;base64,AA":"bottom"}}'),
    { ...DEFAULT_LAYOUT, hidden: { right: false, bottom: true }, tabs: { files: "right", "term:a": "bottom" } });
  // What reads back is its own: changing it leaves the default alone.
  readLayout(null).hidden.right = true;
  assert.equal(DEFAULT_LAYOUT.hidden.right, false);
});

test("a tab dragged to a dock stays there", () => {
  const moved = moveTab({ ...DEFAULT_LAYOUT, hidden: { right: false, bottom: true } }, "term:fix-login", "bottom");
  assert.equal(tabDock(moved, "term:fix-login"), "bottom");
  assert.equal(tabDock(moved, "files"), "right");
  // The dock it went to unfolds.
  assert.equal(moved.hidden.bottom, false);
  assert.deepEqual(readLayout(JSON.stringify(moved)), moved);
  // Both docks show their tabs; a window too narrow for the right dock
  // shows them all at the bottom.
  const ids = ["files", "term:fix-login", "/srv/a.png"];
  assert.deepEqual(splitTabs(moved, ids, 1200), { right: ["files", "/srv/a.png"], bottom: ["term:fix-login"] });
  assert.deepEqual(splitTabs(moved, ids, SIDE_BY_SIDE_MIN - 1), { right: [], bottom: ids });
  // Back to the right: the bottom dock has no tabs, so it goes.
  const back = moveTab(moved, "term:fix-login", "right");
  assert.deepEqual(splitTabs(back, ids, 1200), { right: ids, bottom: [] });
});

test("only the newest tab moves are kept, and none of a picture by its data", () => {
  let l = DEFAULT_LAYOUT;
  for (let i = 0; i < KEPT_TABS + 5; i++) l = moveTab(l, `term:t${i}`, "bottom");
  assert.equal(Object.keys(l.tabs).length, KEPT_TABS);
  assert.equal(tabDock(l, "term:t0"), "right");
  assert.equal(tabDock(l, `term:t${KEPT_TABS + 4}`), "bottom");
  assert.deepEqual(moveTab(DEFAULT_LAYOUT, "data:image/png;base64,AA", "bottom").tabs, {});
  // A move the window does not keep still counts while it is open.
  const m = new Map([["data:image/png;base64,AA", "bottom" as const]]);
  assert.deepEqual(splitTabs(DEFAULT_LAYOUT, ["files", "data:image/png;base64,AA"], 1200, m),
    { right: ["files"], bottom: ["data:image/png;base64,AA"] });
});

test("zooming keeps the point under the pointer in place", () => {
  const v = zoomAt({ scale: 1, x: 0, y: 0, fit: true }, 2, 100, 50);
  assert.deepEqual(v, { scale: 2, x: -100, y: -50, fit: false });
  // The picture point under (100, 50) is the same before and after.
  assert.equal((100 - v.x) / v.scale, 100);
  assert.equal(zoomAt({ scale: ZOOM_MAX, x: 0, y: 0, fit: false }, 2, 0, 0).scale, ZOOM_MAX);
});
