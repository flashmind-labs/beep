import { calls, answers, nativeEvent } from "./bar-env.ts";
import test from "node:test";
import assert from "node:assert/strict";
import { subscribeTopics as subscribeEvents } from "../src/ui/mesh-events.ts";
import { refreshTopicSubscriptions, matchesTopic, subscribeChanges as subscribeTopics, topic, type MeshEvent } from "../src/ui/mesh-events.ts";
import { invoke } from "../src/ui/tauri.ts";
import { clearViews, installViews, snapshotView } from "../src/ui/snapshot-state.ts";

const settle = () => new Promise<void>(resolve => setImmediate(resolve));
type Reader = { id: string; topics: unknown[]; onEvent: { onmessage(event: MeshEvent): void } };

test("resource, namespace and all-topic subscriptions match plugin events", () => {
  const event = topic("plugin.example", "resource");
  assert.equal(matchesTopic(topic("*"), event), true);
  assert.equal(matchesTopic(topic("plugin.example"), event), true);
  assert.equal(matchesTopic(event, event), true);
  assert.equal(matchesTopic(topic("plugin.example", "another"), event), false);
  assert.equal(matchesTopic(topic("files"), event), false);
});

test("direct dotted subscriptions send canonical strings and filter hierarchical events", async () => {
  nativeEvent("center-shown");
  await settle();
  calls.length = 0;
  const exact: number[] = [], scoped: number[] = [];
  const stopExact = subscribeEvents("files.org.repo", event => exact.push(event.revision));
  const stopScoped = subscribeEvents(["files.org.*", "files.%6f%72%67.*"], event => scoped.push(event.revision));
  try {
    await settle();
    const reader = latestReader();
    assert.deepEqual(new Set((reader.topics as Array<{ topic: string }>).map(interest => interest.topic)), new Set(["files.org.repo", "files.org.*"]));
    ["files.org.repo", "files.org.other", "files.organization.repo", "files.*", "files.other.*"].forEach((topic, index) => {
      reader.onEvent.onmessage({ revision: index + 1, machine_id: "machine-a", topic, data: { value: index } });
    });
    assert.deepEqual(exact, [1, 4]);
    assert.deepEqual(scoped, [1, 2, 4]);
  } finally {
    stopExact(); stopScoped();
    await settle();
  }
});

test("invalid dotted patterns do not register native demand", async () => {
  await settle();
  calls.length = 0;
  assert.throws(() => subscribeEvents("files.*.repo", () => {}));
  await settle();
  assert.equal(calls.length, 0);
});

test("views share a reader, filter machines, coalesce bursts and ignore disposed subscriptions", async t => {
  t.mock.timers.enable({ apis: ["setTimeout"] });
  Object.assign(document, { visibilityState: "visible" });
  calls.length = 0;
  let files = 0, plugins = 0;
  const ids: number[] = [];
  const stopEvents = subscribeEvents(topic("plugin.example"), event => ids.push(event.revision));
  const stopFiles = subscribeTopics(topic("files", "/srv/project"), () => files++, "machine-a");
  const stopPlugins = subscribeTopics(topic("plugin.example"), () => plugins++);
  await settle();
  const readers = calls.filter(call => call.cmd === "mesh_watch");
  assert.equal(readers.length, 1);
  const reader = readers[0].args as Reader;
  assert.equal(reader.topics.length, 2);
  let sequence = 0;
  const send = (machine: string, name: string, key: string | null) =>
    reader.onEvent.onmessage({ revision: ++sequence, machine_id: machine, topic: topic(name, key), data: { value: sequence } });
  send("machine-b", "files", "/srv/project");
  send("machine-a", "files", "/srv/another");
  t.mock.timers.tick(100);
  assert.equal(files, 0);
  send("machine-a", "files", "/srv/project");
  for (let i = 0; i < 50; i++) send("machine-a", "files", "/srv/project");
  send("machine-b", "plugin.example", "resource");
  t.mock.timers.tick(100);
  assert.equal(files, 1);
  assert.equal(plugins, 1);
  await settle();
  send("machine-b", "plugin.example", "second");
  send("machine-b", "plugin.example", "third");
  assert.equal(ids.length, 3);
  assert.ok(ids.every((id, index) => index === 0 || id > ids[index - 1]));
  send("machine-a", "files", "/srv/project");
  send("machine-b", "plugin.example", "resource");
  t.mock.timers.tick(100);
  assert.equal(files, 2);
  assert.equal(plugins, 2);
  send("machine-a", "files", "/srv/project");
  stopFiles();
  t.mock.timers.tick(100);
  assert.equal(files, 2);
  stopPlugins();
  stopEvents();
  await settle();
  assert.ok(calls.some(call => call.cmd === "mesh_topics" && (call.args as { topics: unknown }).topics === null));
  send("machine-b", "plugin.example", "late");
  t.mock.timers.tick(100);
  assert.equal(plugins, 2);
});

const latestReader = () => calls.filter(call => call.cmd === "mesh_watch").at(-1)!.args as Reader;
const update = (revision: number, name = "files"): MeshEvent => ({
  revision, machine_id: "machine-a", topic: topic(name), data: { value: revision },
});

test("initial query answers do not trigger rereads while live query updates and topic snapshots do", async t => {
  t.mock.timers.enable({ apis: ["setTimeout"] });
  nativeEvent("center-shown");
  await settle();
  let rereads = 0;
  const received: MeshEvent[] = [];
  const stopState = subscribeTopics(topic("files"), () => rereads++);
  const stopEvents = subscribeEvents(topic("files"), event => received.push(event));
  t.after(() => { stopState(); stopEvents(); });
  await settle();
  const reader = latestReader();
  for (let query = 1; query <= 256; query++) {
    reader.onEvent.onmessage({ ...update(query), query, initial: true });
  }
  t.mock.timers.tick(1000);
  await settle();
  assert.equal(rereads, 0, "reading more folders than the query cache holds must not start a refresh loop");
  assert.equal(received.length, 256, "lossless subscribers still receive every initial answer");
  reader.onEvent.onmessage({ ...update(257), query: 256, initial: false });
  t.mock.timers.tick(100);
  await settle();
  assert.equal(rereads, 1);
  reader.onEvent.onmessage({ ...update(258), query: 256, initial: false, data: { error: "temporary read failure" } });
  t.mock.timers.tick(100);
  await settle();
  assert.equal(rereads, 2, "query errors still notify the view");
  reader.onEvent.onmessage({ ...update(259), initial: true });
  t.mock.timers.tick(100);
  await settle();
  assert.equal(rereads, 3, "authoritative topic snapshots still reconcile the view on reconnect");
});

test("inactive views make no requests and unchanged focus never rereads healthy state", async t => {
  t.mock.timers.enable({ apis: ["setTimeout"] });
  nativeEvent("center-shown");
  await settle();
  calls.length = 0;
  let visible = false, reads = 0;
  const stop = subscribeTopics(topic("files"), () => reads++, "machine-a", () => visible);
  t.after(stop);
  await settle();
  t.mock.timers.tick(86_400_000);
  assert.equal(calls.length, 0);
  visible = true;
  refreshTopicSubscriptions();
  await settle();
  assert.equal(calls.filter(call => call.cmd === "mesh_watch").length, 1);
  const reader = latestReader();
  reader.onEvent.onmessage(update(1));
  t.mock.timers.tick(100);
  await settle();
  assert.equal(reads, 1);
  const requests = calls.length;
  for (let i = 0; i < 10; i++) window.dispatchEvent(new Event("focus"));
  t.mock.timers.tick(86_400_000);
  await settle();
  assert.equal(reads, 1);
  assert.equal(calls.length, requests);
  visible = false;
  refreshTopicSubscriptions();
  await settle();
  assert.equal((calls.at(-1)!.args as { topics: unknown }).topics, null);
  const cancelled = calls.length;
  reader.onEvent.onmessage(update(2));
  await settle();
  assert.equal(reads, 1);
  assert.equal(calls.length, cancelled + 1, "late registration retries cancellation");
});

test("async snapshot application keeps the latest projection while paused or busy", async t => {
  t.mock.timers.enable({ apis: ["setTimeout"] });
  nativeEvent("center-shown");
  await settle();
  calls.length = 0;
  let finish!: () => void;
  const first = new Promise<void>(done => { finish = done; });
  const heard: MeshEvent[] = [];
  const stop = subscribeTopics(topic("files"), async event => {
    heard.push(event);
    if (heard.length === 1) await first;
  });
  t.after(() => { stop(); nativeEvent("center-shown"); });
  await settle();
  const reader = latestReader();
  reader.onEvent.onmessage(update(1));
  reader.onEvent.onmessage(update(2));
  t.mock.timers.tick(100);
  await settle();
  assert.equal(heard.length, 1);
  assert.equal(heard[0].revision, 2, "new snapshots replace earlier snapshots of the same projection");
  for (let i = 3; i <= 30; i++) reader.onEvent.onmessage(update(i));
  t.mock.timers.tick(10_000);
  assert.equal(heard.length, 1, "an in-flight read is never overlapped");
  nativeEvent("center-hidden");
  finish();
  await settle();
  t.mock.timers.tick(86_400_000);
  assert.equal(heard.length, 1);
  nativeEvent("center-shown");
  await settle();
  t.mock.timers.tick(100);
  await settle();
  assert.equal(heard.length, 2);
  assert.equal(heard[1].revision, 30);
  t.mock.timers.tick(86_400_000);
  await settle();
  assert.equal(heard.length, 2, "healthy idle subscriptions have no read timer");
});

test("failed reads retry only while active and independent topics do not reread each other", async t => {
  t.mock.timers.enable({ apis: ["setTimeout"] });
  t.mock.method(console, "warn", () => {});
  nativeEvent("center-shown");
  await settle();
  calls.length = 0;
  let files = 0, reviews = 0, visible = true;
  const stopFiles = subscribeTopics(topic("files"), async () => {
    if (++files === 1) throw new Error("temporary read failure");
  }, null, () => visible);
  const stopReviews = subscribeTopics(topic("reviews"), () => reviews++);
  t.after(() => { stopFiles(); stopReviews(); });
  await settle();
  latestReader().onEvent.onmessage(update(1));
  t.mock.timers.tick(100);
  await settle();
  assert.equal(files, 1);
  assert.equal(reviews, 0);
  visible = false;
  refreshTopicSubscriptions();
  t.mock.timers.tick(86_400_000);
  await settle();
  assert.equal(files, 1);
  latestReader().onEvent.onmessage(update(2, "reviews"));
  t.mock.timers.tick(100);
  await settle();
  assert.equal(reviews, 1);
  assert.equal(files, 1);
  visible = true;
  refreshTopicSubscriptions();
  t.mock.timers.tick(100);
  await settle();
  assert.equal(files, 2);
  t.mock.timers.tick(86_400_000);
  await settle();
  assert.equal(files, 2);
});

test("subscription changes are ordered and unchanged demand does not issue IPC", async t => {
  nativeEvent("center-shown");
  await settle();
  calls.length = 0;
  const stopFiles = subscribeTopics(topic("files"), () => {});
  let finish!: () => void;
  answers.mesh_topics = new Promise<void>(done => { finish = done; });
  const stopReviews = subscribeTopics(topic("reviews"), () => {});
  t.after(() => { delete answers.mesh_topics; finish(); stopFiles(); stopReviews(); });
  await settle();
  // The first combined set is carried by mesh_watch, which has no registration race.
  refreshTopicSubscriptions();
  await settle();
  assert.equal(calls.filter(call => call.cmd === "mesh_topics").length, 0);
  stopFiles();
  await settle();
  assert.equal(calls.filter(call => call.cmd === "mesh_topics").length, 1);
  const stopForge = subscribeTopics(topic("forge"), () => {});
  t.after(stopForge);
  await settle();
  assert.equal(calls.filter(call => call.cmd === "mesh_topics").length, 1, "updates wait for the preceding IPC");
  delete answers.mesh_topics;
  finish();
  await settle();
  const sent = calls.filter(call => call.cmd === "mesh_topics");
  assert.equal(sent.length, 2);
  assert.deepEqual((sent[1].args as { topics: Array<{ topic: string }> }).topics.map(interest => interest.topic), ["forge.*", "reviews.*"]);
  for (let i = 0; i < 10; i++) refreshTopicSubscriptions();
  await settle();
  assert.equal(calls.filter(call => call.cmd === "mesh_topics").length, 2);
});

test("subscription visibility waits until module state is initialized", async t => {
  nativeEvent("center-shown");
  let initialized = false, checked = 0;
  const stop = subscribeTopics(topic("agents"), () => {}, null, () => {
    assert.ok(initialized);
    checked++;
    return false;
  });
  t.after(stop);
  initialized = true;
  await settle();
  assert.ok(checked > 0);
});

test("distinct projections sharing a topic survive coalescing", async t => {
  t.mock.timers.enable({ apis: ["setTimeout"] });
  nativeEvent("center-shown");
  const heard: MeshEvent[] = [];
  const stop = subscribeTopics("files.org.repo", event => heard.push(event));
  t.after(stop);
  await settle();
  const reader = latestReader();
  for (const [query, revision] of [[1, 1], [2, 2], [1, 3]]) reader.onEvent.onmessage({
    machine_id: "machine-a", topic: "files.org.repo", query, revision, data: { value: revision },
  });
  t.mock.timers.tick(100);
  await settle();
  t.mock.timers.tick(100);
  await settle();
  assert.deepEqual(heard.map(event => [event.query, event.revision]), [[2, 2], [1, 3]]);
});

test("controller view snapshots are installed before callbacks and discarded when hidden", async t => {
  nativeEvent("center-shown");
  const heard: unknown[] = [];
  const stop = subscribeEvents("home.*", async () => heard.push(await invoke("center_home")));
  t.after(() => { stop(); nativeEvent("center-shown"); });
  await settle();
  const reader = latestReader();
  calls.length = 0;
  reader.onEvent.onmessage({ machine_id: "controller", topic: "home.*", revision: 1,
    data: { views: [{ command: "center_home", args: {}, value: { machines: ["example"] } }] },
  });
  await settle();
  assert.deepEqual(heard, [{ machines: ["example"] }]);
  assert.equal(calls.some(call => call.cmd === "center_home"), false);
  nativeEvent("home-changed");
  reader.onEvent.onmessage({ machine_id: "controller", topic: "home.*", revision: 2,
    data: { views: [{ command: "center_home", args: {}, value: { machines: ["example"], sessions: ["late"] } }] },
  });
  await settle();
  assert.deepEqual(heard, [{ machines: ["example"] }, { machines: ["example"], sessions: ["late"] }]);
  assert.equal(calls.some(call => call.cmd === "center_home"), false, "late source completion publishes a new projection instead of a UI refetch");
  nativeEvent("center-hidden");
  await settle();
  assert.equal(snapshotView("center_home"), undefined);
});

test("explicit refreshes bypass view snapshots and writes discard stale views", async t => {
  t.mock.timers.enable({ apis: ["setTimeout"] });
  t.after(() => { clearViews(); delete answers.save_agent_token; });
  installViews({ views: [{ command: "center_project_runs", args: { projectId: "example" }, value: { runs: [1] } }] });
  calls.length = 0;
  await invoke("center_project_runs", { projectId: "example", refresh: true });
  assert.equal(calls.filter(call => call.cmd === "center_project_runs").length, 1);
  let finish!: () => void;
  answers.save_agent_token = new Promise<void>(resolve => { finish = resolve; });
  const write = invoke("save_agent_token", { agentSlug: "example", secret: "fixture" });
  installViews({ views: [{ command: "center_project_runs", args: { projectId: "example" }, value: { runs: [1] } }] });
  finish();
  await write;
  assert.equal(snapshotView("center_project_runs", { projectId: "example" }), undefined);
  t.mock.timers.tick(5000);
});

test("source errors invalidate rendered views and retries can read recovered state", async t => {
  t.mock.timers.enable({ apis: ["setTimeout"] });
  t.mock.method(console, "warn", () => {});
  nativeEvent("center-shown");
  const heard: unknown[] = [];
  let errors = 0;
  const stop = subscribeTopics("home.*", async () => {
    try { heard.push(await invoke("center_home")); }
    catch (error) { errors++; throw error; }
  });
  t.after(() => { stop(); delete answers.center_home; });
  await settle();
  const reader = latestReader();
  reader.onEvent.onmessage({ machine_id: "controller", topic: "home.*", revision: 1,
    data: { views: [{ command: "center_home", args: {}, value: { machines: ["before"] } }] },
  });
  t.mock.timers.tick(100);
  await settle();
  reader.onEvent.onmessage({ machine_id: "controller", topic: "home.*", revision: 2,
    data: { views: [{ command: "center_home", args: {}, error: "temporary source failure" }] },
  });
  t.mock.timers.tick(100);
  await settle();
  assert.equal(errors, 1);
  assert.equal(snapshotView("center_home"), undefined);
  answers.center_home = { machines: ["after"] };
  t.mock.timers.tick(1000);
  await settle();
  assert.deepEqual(heard, [{ machines: ["before"] }, { machines: ["after"] }]);
  t.mock.timers.tick(5000);
});

test("a slow view bounds pending projections while preserving errors", async t => {
  t.mock.timers.enable({ apis: ["setTimeout"] });
  nativeEvent("center-shown");
  const heard: MeshEvent[] = [];
  let finish!: () => void;
  const busy = new Promise<void>(resolve => { finish = resolve; });
  const stop = subscribeTopics("files.*", async event => { heard.push(event); if (heard.length === 1) await busy; });
  t.after(stop);
  await settle();
  const reader = latestReader();
  reader.onEvent.onmessage({ machine_id: "node", topic: "files.*", revision: 1, query: 999, data: {} });
  t.mock.timers.tick(100);
  await settle();
  reader.onEvent.onmessage({ machine_id: "node", topic: "files.*", revision: 2, query: 1000, data: { error: "read failed" } });
  for (let query = 0; query < 300; query++) reader.onEvent.onmessage({ machine_id: "node", topic: "files.*", revision: query + 3, query, data: { query } });
  finish();
  await settle();
  for (let index = 0; index < 260; index++) { t.mock.timers.tick(100); await settle(); }
  assert.equal(heard.length, 257);
  assert.equal(heard[1].query, 1000);
  assert.equal(heard.at(-1)!.query, 299);
  assert.equal(heard.some(event => event.query === 0), false);
});

test("pending snapshot payloads respect the byte budget", async t => {
  t.mock.timers.enable({ apis: ["setTimeout"] });
  nativeEvent("center-shown");
  const heard: MeshEvent[] = [];
  let finish!: () => void;
  const busy = new Promise<void>(resolve => { finish = resolve; });
  const stop = subscribeTopics("files.*", async event => { heard.push(event); if (heard.length === 1) await busy; });
  t.after(stop);
  await settle();
  const reader = latestReader(), payload = "x".repeat(1024 * 1024);
  reader.onEvent.onmessage({ machine_id: "node", topic: "files.*", revision: 1, query: 999, data: {} });
  t.mock.timers.tick(100);
  await settle();
  for (let query = 0; query < 10; query++) reader.onEvent.onmessage({ machine_id: "node", topic: "files.*", revision: query + 2, query, data: { payload } });
  finish();
  await settle();
  for (let index = 0; index < 12; index++) { t.mock.timers.tick(100); await settle(); }
  assert.deepEqual(heard.map(event => event.query), [999, 3, 4, 5, 6, 7, 8, 9]);
});
