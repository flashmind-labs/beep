import test from "node:test";
import assert from "node:assert/strict";
import {
  BOTTOM_MIN, CONV_MIN_WIDTH, DEFAULT_LAYOUT, KEPT_TABS, RIGHT_MIN, SIDE_BY_SIDE_MIN, ZOOM_MAX,
  actualView, baseName, clipName, clipTime, dropDock, effectiveDock, fitView, mediaKind, moveTab, paneSize, readLayout, resolvePath,
  splitTabs, tabDock, zoomAt,
} from "../src/media-state.ts";

test("files are shown by their extension, in any case", () => {
  assert.equal(mediaKind("shots/a.PNG"), "image");
  assert.equal(mediaKind("out/walk.mp4"), "video");
  assert.equal(mediaKind("clip.MOV"), "video");
  assert.equal(mediaKind("model/turntable.glb"), "model");
  assert.equal(mediaKind("a.gltf"), "model");
  assert.equal(mediaKind("a.obj"), "model");
  assert.equal(mediaKind("part.stl"), "model");
  assert.equal(mediaKind("sheet.pdf"), "pdf");
  assert.equal(mediaKind("src/main.rs"), null);
  assert.equal(mediaKind(".mp4"), null);
  assert.equal(mediaKind("dir.mp4/file"), null);
});

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

test("a tab dropped by the right or bottom edge docks there", () => {
  const shell = { left: 100, top: 50, width: 1000, height: 600 };
  assert.equal(dropDock(shell, 1050, 200, true), "right");
  assert.equal(dropDock(shell, 500, 600, true), "bottom");
  assert.equal(dropDock(shell, 500, 300, true), null);
  // Outside the session.
  assert.equal(dropDock(shell, 1150, 200, true), null);
  // In the corner, the nearer edge.
  assert.equal(dropDock(shell, 1095, 560, true), "right");
  assert.equal(dropDock(shell, 960, 645, true), "bottom");
  // No right dock fits: the right edge takes nothing.
  assert.equal(dropDock(shell, 1050, 200, false), null);
  assert.equal(dropDock(shell, 1050, 600, false), "bottom");
});

test("a narrow window puts the pane at the bottom", () => {
  assert.equal(effectiveDock("right", SIDE_BY_SIDE_MIN), "right");
  assert.equal(effectiveDock("right", SIDE_BY_SIDE_MIN - 1), "bottom");
  assert.equal(effectiveDock("bottom", 2000), "bottom");
});

test("the pane's size stays between its least and the conversation's room", () => {
  assert.equal(paneSize("right", null, 1000), 420);
  assert.equal(paneSize("right", 100, 1000), RIGHT_MIN);
  assert.equal(paneSize("right", 900, 1000), 1000 - CONV_MIN_WIDTH);
  assert.equal(paneSize("bottom", 50, 800), BOTTOM_MIN);
  assert.equal(paneSize("bottom", null, 800), 320);
  // No room at all: the pane keeps its least.
  assert.equal(paneSize("right", 500, 500), RIGHT_MIN);
});

test("fit shrinks a large picture into its box and leaves a small one", () => {
  assert.deepEqual(fitView(2000, 1000, 500, 500), { scale: 0.25, x: 0, y: 125, fit: true });
  assert.deepEqual(fitView(100, 50, 500, 500), { scale: 1, x: 200, y: 225, fit: true });
  assert.deepEqual(actualView(2000, 1000, 500, 500), { scale: 1, x: 0, y: 0, fit: false });
});

test("zooming keeps the point under the pointer in place", () => {
  const v = zoomAt({ scale: 1, x: 0, y: 0, fit: true }, 2, 100, 50);
  assert.deepEqual(v, { scale: 2, x: -100, y: -50, fit: false });
  // The picture point under (100, 50) is the same before and after.
  assert.equal((100 - v.x) / v.scale, 100);
  assert.equal(zoomAt({ scale: ZOOM_MAX, x: 0, y: 0, fit: false }, 2, 0, 0).scale, ZOOM_MAX);
});

test("a clip without a name is named by its place", () => {
  assert.deepEqual(["Walk", "", "  "].map(clipName), ["Walk", "Clip 2", "Clip 3"]);
});

test("clip times read minutes, seconds and tenths", () => {
  assert.equal(clipTime(0), "0:00.0");
  assert.equal(clipTime(1.25), "0:01.2");
  assert.equal(clipTime(0.3), "0:00.3");
  assert.equal(clipTime(59.99), "0:59.9");
  assert.equal(clipTime(125.5), "2:05.5");
  assert.equal(clipTime(-1), "0:00.0");
});
