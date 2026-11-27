import test from "node:test";
import assert from "node:assert/strict";
import { followedProject, makeProjectLive } from "../src/stores/project/projectlive.ts";
function setup() {
  const calls: [string, unknown][] = [];
  const reads: [string, boolean, boolean][] = [];
  const invoke = (async (cmd: string, args?: unknown) => { calls.push([cmd, args]); }) as never;
  const live = makeProjectLive({ invoke, redraw: () => {}, reload: (id, update) => reads.push([id, update.lists, update.runs]) });
  return { live, calls, reads };
}

test("the window follows only the project it shows", async () => {
  const { live, calls } = setup();
  live.follow("p1");
  live.follow("p1");
  live.follow("p2");
  await live.follow(null);
  assert.deepEqual(calls, [
    ["follow_project", { projectId: "p1" }],
    ["unfollow_project", { projectId: "p1" }],
    ["follow_project", { projectId: "p2" }],
    ["unfollow_project", { projectId: "p2" }],
  ]);
});

test("an event reads again only what it changed, and the chip says live or polling", async () => {
  const { live, reads } = setup();
  await live.follow("p1");
  assert.equal(live.mode("p1"), null, "unknown until the desktop says");
  live.heard({ project_id: "p1", live: true, polling_why: null, lists: false, runs: false });
  assert.deepEqual(reads, []);
  assert.deepEqual(live.mode("p1"), { live: true, why: null });
  live.heard({ project_id: "p1", live: true, polling_why: null, lists: false, runs: true });
  assert.deepEqual(reads, [["p1", false, true]]);
  live.heard({ project_id: "p1", live: false, polling_why: "Live updates need admin rights.", lists: false, runs: false });
  assert.deepEqual(live.mode("p1"), { live: false, why: "Live updates need admin rights." });
});

test("an update hands over the views it pushed, found by their arguments", async () => {
  const seen: unknown[] = [];
  const invoke = (async () => {}) as never;
  const live = makeProjectLive({ invoke, redraw() {}, reload: (_, update) => seen.push(
    update.view("center_project_runs", { projectId: "p1" }),
    update.view("center_project_run", { projectId: "p1", runId: 7, host: null }),
    update.view("center_project_run", { projectId: "p1", runId: 8, host: null }),
    update.view("center_project_lists", { projectId: "p1" }),
  ) });
  await live.follow("p1");
  live.heard({ project_id: "p1", live: true, polling_why: null, lists: false, runs: true, views: [
    { command: "center_project_runs", args: { projectId: "p1" }, value: { runs: [] } },
    { command: "center_project_run", args: { projectId: "p1", runId: 7, host: null }, error: "gone" },
  ] });
  assert.deepEqual(seen, [{ value: { runs: [] } }, { error: "gone" }, null, null]);
});

test("hidden and unrelated project snapshots do not read views", async () => {
  const { live, reads } = setup();
  await live.follow("p1");
  live.heard({ project_id: "p2", live: true, polling_why: null, lists: true, runs: true });
  const hidden = live.follow(null);
  live.heard({ project_id: "p1", live: true, polling_why: null, lists: true, runs: true });
  await hidden;
  assert.deepEqual(reads, []);
});

const settle = () => new Promise<void>(done => setImmediate(done));
const deferred = () => {
  let resolve!: () => void;
  let reject!: (error: Error) => void;
  const promise = new Promise<void>((done, fail) => { resolve = done; reject = fail; });
  return { promise, resolve, reject };
};

function delayed() {
  const first = deferred();
  const calls: Array<[string, string]> = [];
  const registered = new Set<string>();
  const invoke = (async (command: string, args: { projectId: string }) => {
    calls.push([command, args.projectId]);

    if (command === "follow_project") {
      if (args.projectId === "p1") await first.promise;
      registered.add(args.projectId);
    } else {
      registered.delete(args.projectId);
    }
  }) as never;
  const live = makeProjectLive({ invoke, redraw() {}, reload() {} });
  return { live, first, calls, registered };
}

test("hiding waits for delayed registration before releasing its source", async () => {
  const { live, first, calls, registered } = delayed();
  live.follow("p1");
  await settle();
  const hidden = live.follow(null);
  await settle();
  assert.deepEqual(calls, [["follow_project", "p1"]]);
  first.resolve();
  await hidden;
  assert.deepEqual(calls, [["follow_project", "p1"], ["unfollow_project", "p1"]]);
  assert.equal(registered.size, 0);
});

test("switching then hiding cannot overtake a delayed project follow", async () => {
  const { live, first, calls, registered } = delayed();
  live.follow("p1");
  await settle();
  live.follow("p2");
  const hidden = live.follow(null);
  await settle();
  assert.deepEqual(calls, [["follow_project", "p1"]]);
  first.resolve();
  await hidden;
  assert.deepEqual(calls, [
    ["follow_project", "p1"], ["unfollow_project", "p1"],
    ["follow_project", "p2"], ["unfollow_project", "p2"],
  ]);
  assert.equal(registered.size, 0);
});

test("failed registration and cleanup preserve later ownership changes", async t => {
  t.mock.method(console, "warn", () => {});
  const first = deferred();
  const calls: Array<[string, string]> = [];
  const registered = new Set<string>();
  let cleanupAttempts = 0;
  const invoke = (async (command: string, args: { projectId: string }) => {
    const id = args.projectId;
    calls.push([command, id]);

    if (command === "follow_project") {
      registered.add(id);
      if (id === "p1") await first.promise;
    } else {
      if (id === "p1" && ++cleanupAttempts === 1) throw new Error("cleanup reply failed");
      registered.delete(id);
    }
  }) as never;
  const live = makeProjectLive({ invoke, redraw() {}, reload() {} });
  live.follow("p1");
  await settle();
  const switched = live.follow("p2");
  first.reject(new Error("registration reply failed"));
  await switched;
  assert.deepEqual([...registered], ["p2"], "failed cleanup is retried before switching");
  await live.follow(null);
  assert.equal(registered.size, 0);
  assert.deepEqual(calls, [
    ["follow_project", "p1"], ["unfollow_project", "p1"], ["unfollow_project", "p1"],
    ["follow_project", "p2"], ["unfollow_project", "p2"],
  ]);
});

test("repeated cleanup retries an unacknowledged source without starting it again", async t => {
  t.mock.method(console, "warn", () => {});
  let cleanups = 0;
  const calls: string[] = [];
  const invoke = (async (command: string) => {
    calls.push(command);
    if (command === "unfollow_project" && ++cleanups === 1) throw new Error("cleanup failed");
  }) as never;
  const live = makeProjectLive({ invoke, redraw() {}, reload() {} });
  await live.follow("p1");
  await live.follow(null);
  await live.follow(null);
  await live.follow(null);
  assert.deepEqual(calls, ["follow_project", "unfollow_project", "unfollow_project"]);
});

test("followedProject follows the visible imported project page or rail while shown", () => {
  const projects = [{ name: "acme/app", imported: true }, { name: "acme/local", imported: false }];
  assert.equal(followedProject(true, "projects", "acme/app", null, projects), "acme/app");
  assert.equal(followedProject(true, "pane", null, "acme/app", projects), "acme/app");
  assert.equal(followedProject(true, "projects", null, "acme/app", projects), "acme/app");
  assert.equal(followedProject(true, "pane", "acme/app", null, projects), null);
  assert.equal(followedProject(true, "home", "acme/app", null, projects), null);
  assert.equal(followedProject(false, "pane", null, "acme/app", projects), null);
  assert.equal(followedProject(true, "projects", "acme/local", null, projects), null);
  assert.equal(followedProject(true, "pane", null, "acme/local", projects), null);
  assert.equal(followedProject(true, "pane", null, "acme/app", null), null);
});
