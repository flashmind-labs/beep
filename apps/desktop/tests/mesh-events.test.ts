import { calls, answers, nativeEvent } from "./bar-env.ts";
import test from "node:test";
import assert from "node:assert/strict";
import { subscribeTopics as subscribeEvents } from "../src/ui/mesh-events.ts";
import { refreshTopicSubscriptions, matchesTopic, subscribeChanges as subscribeTopics, topic, type MeshEvent } from "../src/ui/mesh-events.ts";

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

test("views share a reader, filter machines, coalesce bursts and ignore disposed subscriptions", async t => {
  t.mock.timers.enable({ apis: ["setTimeout"] });
  Object.assign(document, { visibilityState: "visible" });
  calls.length = 0;
  let files = 0, plugins = 0;
  const ids: number[] = [];
  const stopEvents = subscribeEvents(topic("plugin.example"), event => ids.push(event.id.sequence));
  const stopFiles = subscribeTopics(topic("files", "/srv/project"), () => files++, "machine-a");
  const stopPlugins = subscribeTopics(topic("plugin.example"), () => plugins++);
  await settle();
  const readers = calls.filter(call => call.cmd === "mesh_watch");
  assert.equal(readers.length, 1);
  const reader = readers[0].args as Reader;
  assert.equal(reader.topics.length, 2);
  let sequence = 0;
  const send = (machine: string, name: string, key: string | null, resync = false) =>
    reader.onEvent.onmessage({ id: { epoch: "example", sequence: ++sequence }, machine_id: machine, topic: topic(name, key), resync });
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
  send("machine-a", "*", null, true);
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
const update = (sequence: number, name = "files", resync = false): MeshEvent => ({
  id: { epoch: "example", sequence }, machine_id: "machine-a", topic: topic(name), resync,
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

test("async reads keep dirty updates and resync until completion, pausing when hidden", async t => {
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
  reader.onEvent.onmessage(update(1, "*", true));
  reader.onEvent.onmessage(update(2));
  t.mock.timers.tick(100);
  await settle();
  assert.equal(heard.length, 1);
  assert.equal(heard[0].resync, true, "ordinary updates cannot erase pending resync");
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
  assert.equal(heard[1].id.sequence, 30);
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
  assert.deepEqual((sent[1].args as { topics: Array<{ topic: { name: string } }> }).topics.map(interest => interest.topic.name), ["forge", "reviews"]);
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
