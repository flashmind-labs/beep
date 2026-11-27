import test from "node:test";
import assert from "node:assert/strict";
import { makeFileSync, type Conflict } from "../src/parts/project/filesync.ts";

type Deps = Parameters<typeof makeFileSync>[0];

const conflict: Conflict = { path: "a.txt", original: "a.txt", on: [], resolver: "m1", holder: null };

test("a refused sync action shows its error, and an action reads no detail: the push brings it", async () => {
  const calls: string[] = [];
  const invoke = (async (command: string) => {
    calls.push(command);
    if (command === "project_sync_resolve" && calls.length === 1) throw new Error("no machine can settle it");
    return [];
  }) as Deps["invoke"];
  const sync = makeFileSync({ invoke, redraw() {} });

  await sync.resolve("p", conflict, true);
  assert.match(sync.page("p").error, /no machine can settle it/);
  await sync.resolve("p", conflict, true);
  assert.equal(sync.page("p").error, "");
  // Only the first look at the page reads the detail.
  assert.equal(calls.filter(call => call === "project_sync_detail").length, 1);
});
