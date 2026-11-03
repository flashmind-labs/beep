import test from "node:test";
import assert from "node:assert/strict";
import { eachLimited, foundMatches, tickable, toImport, toggleAllShown } from "../src/addproject-state.ts";
import type { ImportState } from "../src/addproject-state.ts";

const widget = {
  name: "acme/widget",
  remote: "git@example.com:acme/widget.git",
  places: [{ machine_id: "m1", machine_name: "build-box", folder: "/srv/project/widget" }],
  agents: [{ agent: "claude", sessions: 4 }],
};

test("the search matches repo, folder, machine and agent, every word", () => {
  for (const q of ["", "WIDGET", "srv/project", "build-box", "claude code", "claude", "acme build"]) {
    assert.ok(foundMatches(widget, q), q);
  }
  assert.ok(!foundMatches(widget, "relay"));
  assert.ok(!foundMatches(widget, "widget mac-box"));
});

test("done and importing rows cannot be ticked or imported again", () => {
  const status = new Map<string, ImportState>([
    ["a", { state: "done" }],
    ["b", { state: "importing" }],
    ["c", { state: "failed", why: "offline" }],
  ]);
  assert.ok(!tickable(status.get("a")));
  assert.ok(!tickable(status.get("b")));
  assert.ok(tickable(status.get("c")));
  assert.deepEqual(toImport(["a", "b", "c", "d", "e"], new Set(["a", "b", "c", "e"]), status), ["c", "e"]);
});

test("select all shown ticks the shown rows, then unticks them, and leaves hidden ones", () => {
  const status = new Map<string, ImportState>([["done", { state: "done" }]]);
  const on = toggleAllShown(["a", "b", "done"], new Set(["hidden"]), status);
  assert.deepEqual([...on].sort(), ["a", "b", "hidden"]);
  const off = toggleAllShown(["a", "b", "done"], on, status);
  assert.deepEqual([...off], ["hidden"]);
});

test("a batch runs at most the limit at once and a failure leaves the rest going", async () => {
  let running = 0, most = 0;
  const done: number[] = [];
  const gates = new Map<number, () => void>();
  const started: number[] = [];
  const batch = eachLimited([1, 2, 3, 4, 5], 3, item => new Promise<void>((resolve, reject) => {
    running++;
    most = Math.max(most, running);
    started.push(item);
    gates.set(item, () => {
      running--;
      if (item === 2) reject(new Error("offline"));
      else { done.push(item); resolve(); }
    });
  }));
  const settle = () => new Promise(resolve => setImmediate(resolve));
  await settle();
  assert.deepEqual(started, [1, 2, 3]);
  gates.get(2)!();
  await settle();
  assert.deepEqual(started, [1, 2, 3, 4]);
  for (const item of [1, 3, 4]) gates.get(item)!();
  await settle();
  gates.get(5)!();
  await batch;
  assert.equal(most, 3);
  assert.deepEqual(done.sort(), [1, 3, 4, 5]);
});
