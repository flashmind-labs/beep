import { test } from "node:test";
import assert from "node:assert/strict";
import { arrange, openedIn, pickedInOrder, planStarts, startLabel } from "../src/project/openlist-state.ts";

// Wednesday 2026-11-04, 10:00 local.
const now = new Date(2026, 10, 4, 10).getTime();
const at = (d: number, h = 12) => new Date(2026, 10, d, h).getTime();

test("today and yesterday are calendar days in local time", () => {
  assert.equal(openedIn(at(4, 0), "today", now), true);
  assert.equal(openedIn(at(3, 23), "today", now), false);
  assert.equal(openedIn(at(3, 23), "yesterday", now), true);
  assert.equal(openedIn(at(2, 23), "yesterday", now), false);
  assert.equal(openedIn(at(4, 1), "yesterday", now), false);
});

test("this week starts on Monday", () => {
  assert.equal(openedIn(at(2, 0), "week", now), true);
  assert.equal(openedIn(at(1, 23), "week", now), false);
  assert.equal(openedIn(at(4, 9), "week", now), true);
});

test("a row with no time stays under Any time only", () => {
  for (const w of ["today", "yesterday", "week"] as const) assert.equal(openedIn(null, w, now), false);
  assert.equal(openedIn(0, "any", now), true);
});

test("arrange sorts by open time and puts unknown times last", () => {
  const items = [{ n: 1, t: at(2) }, { n: 2, t: null }, { n: 3, t: at(4) }, { n: 4, t: at(3) }];
  const order = (o: "newest" | "oldest") => arrange(items, i => i.t, "any", o, now).map(i => i.n);
  assert.deepEqual(order("newest"), [3, 4, 1, 2]);
  assert.deepEqual(order("oldest"), [1, 4, 3, 2]);
  assert.deepEqual(arrange(items, i => i.t, "week", "newest", now).map(i => i.n), [3, 4, 1]);
});

test("picked issues keep the order shown", () => {
  const shown = [{ key: "a" }, { key: "b" }, { key: "c" }];
  assert.deepEqual(pickedInOrder(shown, new Set(["c", "a", "gone"])).map(i => i.key), ["a", "c"]);
  assert.equal(startLabel(1), "Start 1 agent");
  assert.equal(startLabel(3), "Start 3 agents");
});

test("each issue starts in its own project on a machine that has the checkout", () => {
  const hosts = [{ machine_id: "m1", machine_name: "build-box", agents: ["codex", "claude"] }, { machine_id: "m2", machine_name: "studio", agents: ["codex"] }];
  const places = (p: string) => ({ "acme/api": [{ machine: "m1", cwd: "/srv/api" }], "acme/web": [{ machine: "m2", cwd: "/srv/web" }], "acme/old": [{ machine: "m3", cwd: "/srv/old" }] }[p] ?? []);
  const e = (project: string | null, number: number) => ({ key: String(number), project, number, title: "t", url: "u" });
  const rows = planStarts([e("acme/api", 1), e("acme/web", 2), e("acme/old", 3), e(null, 4)], places, hosts, () => "auto", "claude", ["claude", "codex"]);
  assert.deepEqual(rows.map(r => (r.skip === null ? [r.machine_id, r.agent, r.cwd] : r.skip)), [
    ["m1", "claude", "/srv/api"],
    ["m2", "codex", "/srv/web"],
    "no machine has a checkout",
    "not imported",
  ]);
});
