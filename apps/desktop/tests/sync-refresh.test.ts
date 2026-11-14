import test, { mock } from "node:test";
import assert from "node:assert/strict";
import { makeFileSync, mergeSyncRows, type SyncDetail, type SyncRow } from "../src/project/filesync.ts";

type Deps = Parameters<typeof makeFileSync>[0];

const row = (project: string, machine_id: string, over: Partial<SyncRow> = {}): SyncRow =>
  ({ project, machine_id, status: "in_sync", files: 10, pending: 0, error: null, ...over });

const detailOf = (project: string): SyncDetail => ({ project, pinned: false, paused: false, machines: [] });

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

test("identical summaries keep the same rows and change no project", () => {
  const before = [row("a", "m1"), row("b", "m1"), row("b", "m2")];
  const { rows, changed } = mergeSyncRows(before, before.map(r => ({ ...r })));
  assert.equal(rows, before);
  assert.equal(changed.size, 0);
});

test("one changed row changes only its project and keeps the other rows", () => {
  const before = [row("a", "m1"), row("b", "m1"), row("c", "m2")];
  const { rows, changed } = mergeSyncRows(before, [row("a", "m1"), row("b", "m1", { pending: 3 }), row("c", "m2")]);
  assert.deepEqual([...changed], ["b"]);
  assert.equal(rows[0], before[0]);
  assert.equal(rows[2], before[2]);
  assert.notEqual(rows[1], before[1]);
});

test("added and removed machines change their projects", () => {
  const before = [row("a", "m1"), row("b", "m1")];
  const { changed } = mergeSyncRows(before, [row("a", "m1"), row("a", "m2")]);
  assert.deepEqual([...changed].sort(), ["a", "b"]);
});

test("one summary change reads the detail of that project only", async () => {
  const { sync, details } = counted();
  await Promise.all(["a", "b", "c"].map(p => sync.ensure(p, 0)));
  assert.equal(details().length, 3);

  const before = [row("a", "m1"), row("b", "m1"), row("c", "m1")];
  const { changed } = mergeSyncRows(before, [row("a", "m1"), row("b", "m1", { files: 11 }), row("c", "m1")]);
  mock.timers.enable({ apis: ["Date", "setTimeout"], now: Date.now() });
  await sync.follow(changed, [], 60_000);
  assert.equal(details().length, 3);
  mock.timers.tick(60_000);
  await new Promise(resolve => setImmediate(resolve));
  assert.deepEqual(details().slice(3), ["project_sync_detail:b"]);
  mock.timers.reset();
});

test("a busy sync reads a changed project once per bound, then once after", async () => {
  const { sync, details } = counted();
  mock.timers.enable({ apis: ["Date", "setTimeout"], now: Date.now() });
  await sync.follow(new Set(["a"]), [], 10_000);
  for (let i = 0; i < 10; i++) {
    mock.timers.tick(500);
    await sync.follow(new Set(["a"]), [], 10_000);
  }
  assert.equal(details().length, 1);
  mock.timers.tick(10_000);
  await new Promise(resolve => setImmediate(resolve));
  assert.equal(details().length, 2);
  mock.timers.reset();
});

test("an unchanged summary reads nothing and draws nothing while details are fresh", async () => {
  const { sync, details, redraws } = counted();
  await sync.ensure("a", 0);
  const drawn = redraws();
  const reads = details().length;

  const before = [row("a", "m1")];
  const { changed } = mergeSyncRows(before, [row("a", "m1")]);
  for (let i = 0; i < 20; i++) await sync.follow(changed, ["a"], 60_000);
  assert.equal(details().length, reads);
  assert.equal(redraws(), drawn);
});

test("a viewed project is read again once its detail is older than the bound", async () => {
  const { sync, details } = counted();
  mock.timers.enable({ apis: ["Date"], now: Date.now() });
  sync.watch("a");
  await sync.ensure("a", 0);
  await sync.follow(new Set(), [], 1000);
  assert.equal(details().length, 1);
  mock.timers.tick(1001);
  await sync.follow(new Set(), [], 1000);
  assert.deepEqual(details(), ["project_sync_detail:a", "project_sync_detail:a"]);
  sync.stop();
  mock.timers.tick(1001);
  await sync.follow(new Set(), [], 1000);
  assert.equal(details().length, 2);
  mock.timers.reset();
});

test("a read that finds the same detail keeps its object and draws nothing", async () => {
  const { sync, redraws } = counted();
  await sync.ensure("a", 0);
  const first = sync.detail("a");
  const drawn = redraws();
  await sync.follow(new Set(["a"]), [], 0);
  assert.equal(sync.detail("a"), first);
  assert.equal(redraws(), drawn);
});
