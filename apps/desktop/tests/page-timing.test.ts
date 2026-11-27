import { test } from "node:test";
import assert from "node:assert/strict";
import { PageTiming, pageName } from "../src/stores/center/page-timing.ts";

test("only the visible, current navigation is recorded after two frames", () => {
  let now = 0, visible = true;
  const frames: (() => void)[] = [], samples: [string, number][] = [];
  const timer = new PageTiming(() => now, cb => frames.push(cb), () => visible, (page, ms) => samples.push([page, ms]));
  timer.start("home")();
  frames.shift()!();
  now = 20;
  timer.start("agent")();
  frames.shift()!(); // Home's second frame is superseded.
  frames.shift()!();
  now = 45;
  frames.shift()!();
  assert.deepEqual(samples, [["agent", 25]]);
  timer.start("projects")();
  frames.shift()!();
  visible = false;
  frames.shift()!();
  assert.equal(samples.length, 1);
});

test("page labels never include project or session arguments", () => {
  assert.equal(pageName("pane", { agent: { id: "private" } }), "agent");
  assert.equal(pageName("pane", { draft: { project: "private" } }), "agent");
  assert.equal(pageName("pane", "private"), "pane");
  assert.equal(pageName("projects", { project: "private" }), "projects");
  assert.equal(pageName("pane", "agents"), "agents");
  assert.equal(pageName("pane", "settings"), "settings");
});

test("the view selected by the completed mount names the timing", () => {
  const frames: (() => void)[] = [], samples: string[] = [];
  const timer = new PageTiming(() => 1, cb => frames.push(cb), () => true, page => samples.push(page));
  timer.start("pane")("chats");
  while (frames.length) frames.shift()!();
  assert.deepEqual(samples, ["chats"]);
});

test("a page whose async mount finishes after navigation is not recorded", () => {
  const frames: (() => void)[] = [], samples: string[] = [];
  const timer = new PageTiming(() => 1, cb => frames.push(cb), () => true, page => samples.push(page));
  const old = timer.start("pane");
  timer.start("home")();
  old();
  while (frames.length) frames.shift()!();
  assert.deepEqual(samples, ["home"]);
});
