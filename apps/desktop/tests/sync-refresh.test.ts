import test from "node:test";
import assert from "node:assert/strict";
import { makeFileSync, mergeSyncRows, type SyncDetail, type SyncRow } from "../src/project/filesync.ts";

type Deps = Parameters<typeof makeFileSync>[0];

const row = (project: string, machine_id: string, over: Partial<SyncRow> = {}): SyncRow =>
  ({ project, machine_id, status: "in_sync", files: 10, pending: 0, error: null, ...over });

const detailOf = (project: string, over: Partial<SyncDetail> = {}): SyncDetail => ({ project, pinned: false, paused: false, machines: [], ...over });

/// A file sync whose reads are counted by command and project.
function counted() {
  const reads: string[] = [];
  let redraws = 0;
  const invoke = (async (command: string, args?: { project?: string }) => {
    reads.push(args?.project ? `${command}:${args.project}` : command);
    return command === "project_sync_detail" ? detailOf(args?.project ?? "") : [];
  }) as Deps["invoke"];
  const sync = makeFileSync({ invoke, redraw() { redraws += 1; } });
  return { sync, reads, details: () => reads.filter(r => r.startsWith("project_sync_detail")), redraws: () => redraws };
}

test("added and removed machines change their projects", () => {
  const before = [row("a", "m1"), row("b", "m1")];
  const { rows, changed } = mergeSyncRows(before, [row("a", "m1"), row("a", "m2")]);
  assert.deepEqual(rows.map(({ project, machine_id }) => [project, machine_id]), [["a", "m1"], ["a", "m2"]]);
  assert.deepEqual([...changed].sort(), ["a", "b"]);
});

test("a project's detail is read once, then comes with the sync updates", async () => {
  const { sync, details, redraws } = counted();
  await Promise.all(["a", "b", "c"].map(p => sync.ensure(p)));
  assert.equal(details().length, 3);

  for (let i = 0; i < 20; i++) await Promise.all(["a", "b", "c"].map(p => sync.ensure(p)));
  assert.equal(details().length, 3, "ensuring a project already read asks nothing");

  const drawn = redraws();
  sync.take("b", detailOf("b", { paused: true }));
  assert.equal(sync.detail("b")?.paused, true);
  assert.equal(redraws(), drawn + 1);
  assert.equal(details().length, 3);
});

test("a pushed detail that is the one held keeps its object and draws nothing", async () => {
  const { sync, redraws } = counted();
  await sync.ensure("a");
  const first = sync.detail("a");
  const drawn = redraws();
  sync.take("a", detailOf("a"));
  assert.equal(sync.detail("a"), first);
  assert.equal(redraws(), drawn);
});
