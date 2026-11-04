import test from "node:test";
import assert from "node:assert/strict";
import { makeProjectLive } from "../src/project/projectlive.ts";
import type { MakeEl } from "../src/project/projects-state.ts";

interface Fake { cls: string; text: string; title: string }
const h = ((_tag: string, cls: string, ...kids: string[]) => ({ cls, text: kids.join(""), title: "" })) as unknown as MakeEl;

function setup() {
  const calls: [string, unknown][] = [];
  const reads: [string, boolean, boolean][] = [];
  const invoke = (async (cmd: string, args?: unknown) => { calls.push([cmd, args]); }) as never;
  const live = makeProjectLive({ invoke, h, redraw: () => {}, reload: (id, lists, runs) => reads.push([id, lists, runs]) });
  return { live, calls, reads };
}

test("the window follows only the project it shows", () => {
  const { live, calls } = setup();
  live.follow("p1");
  live.follow("p1");
  live.follow("p2");
  live.follow(null);
  assert.deepEqual(calls, [
    ["follow_project", { projectId: "p1" }],
    ["unfollow_project", { projectId: "p1" }],
    ["follow_project", { projectId: "p2" }],
    ["unfollow_project", { projectId: "p2" }],
  ]);
});

test("an event reads again only what it changed, and the chip says live or polling", () => {
  const { live, reads } = setup();
  assert.equal(live.chip("p1"), null, "unknown until the server says");
  live.heard({ project_id: "p1", live: true, polling_why: null, lists: false, runs: false });
  assert.deepEqual(reads, []);
  assert.equal((live.chip("p1") as unknown as Fake).text, "live");
  live.heard({ project_id: "p1", live: true, polling_why: null, lists: false, runs: true });
  assert.deepEqual(reads, [["p1", false, true]]);
  live.heard({ project_id: "p1", live: false, polling_why: "Live updates need admin rights.", lists: false, runs: false });
  const chip = live.chip("p1") as unknown as Fake;
  assert.equal(chip.text, "polling");
  assert.match(chip.title, /admin rights/);
  // A server too old to say shows no chip.
  live.heard({ project_id: "p1", live: null, polling_why: null, lists: false, runs: false });
  assert.equal(live.chip("p1"), null);
});
