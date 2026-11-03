import test from "node:test";
import assert from "node:assert/strict";
import {
  BOTTOM_MIN, CONV_MIN_WIDTH, DEFAULT_LAYOUT, RIGHT_MIN, SIDE_BY_SIDE_MIN, ZOOM_MAX,
  actualView, baseName, effectiveDock, fitView, mediaKind, paneSize, readLayout, resolvePath, zoomAt,
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
  assert.deepEqual(readLayout('{"dock":"bottom","right":412.4,"bottom":-3,"hidden":true}'),
    { dock: "bottom", right: 412, bottom: null, hidden: true });
  assert.deepEqual(readLayout('{"dock":"left","right":"wide"}'), DEFAULT_LAYOUT);
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
