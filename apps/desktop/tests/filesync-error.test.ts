import test from "node:test";
import assert from "node:assert/strict";
import { makeFileSync, type Conflict, type SyncDetail } from "../src/project/filesync.ts";

type Deps = Parameters<typeof makeFileSync>[0];

test("a refused sync action keeps its error after the detail is read again", async () => {
  const detail: SyncDetail = { project: "p", pinned: false, paused: false, machines: [] };
  const invoke = (async (command: string) => {
    if (command === "project_sync_resolve") throw new Error("no machine can settle it");
    return command === "project_sync_detail" ? detail : [];
  }) as Deps["invoke"];
  const sync = makeFileSync({ invoke, redraw() {} });
  const conflict: Conflict = { path: "a.txt", original: "a.txt", on: [], resolver: "m1", holder: null };

  await sync.resolve("p", conflict, true);
  assert.match(sync.page("p").error, /no machine can settle it/);
});
