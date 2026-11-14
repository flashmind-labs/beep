import test from "node:test";
import assert from "node:assert/strict";
import { connectPluginTopics, connectPlugins, meshTopics, parsePlacements, parseWanted, placementsFor, pluginsStore, publishUi } from "../src/plugins/plugins-state.ts";
import type { WantedTopic } from "../src/plugins/plugins-state.ts";

const image = { kind: "image", src: "data:image/png;base64,AAAA", width: 60, height: 48, pixelated: true };
const placement = { plugin: "composer-cat", slot: "composer", align: "end", offset: 84, sink: 3, node: image };

test("a valid placement is read as sent", () => {
  assert.deepEqual(parsePlacements([placement]), [placement]);
});

test("text and nested rows are read", () => {
  const node = { kind: "row", gap: 4, children: [{ kind: "text", text: "hi" }, image] };
  assert.deepEqual(parsePlacements([{ ...placement, node }]), [{ ...placement, node }]);
});

test("a payload that is not a list is empty", () => {
  for (const payload of [null, {}, "x", 3, undefined]) assert.deepEqual(parsePlacements(payload), []);
});

test("invalid placements are dropped and the rest kept", () => {
  const bad = [
    { ...placement, slot: "sidebar" },
    { ...placement, align: "middle" },
    { ...placement, offset: -1 },
    { ...placement, sink: 1.5 },
    { ...placement, plugin: 4 },
    { ...placement, node: { ...image, src: "https://example.com/cat.png" } },
    { ...placement, node: { ...image, src: "javascript:alert(1)" } },
    { ...placement, node: { ...image, width: "60" } },
    { ...placement, node: { ...image, pixelated: 1 } },
    { ...placement, node: { kind: "script" } },
    { ...placement, node: { kind: "row", gap: 2, children: [{ kind: "text" }] } },
    null,
    "placement",
  ];
  assert.deepEqual(parsePlacements([...bad, placement]), [placement]);
});

test("rows nested too deep are refused", () => {
  let node: unknown = { kind: "text", text: "deep" };
  for (let i = 0; i < 12; i++) node = { kind: "row", gap: 0, children: [node] };
  assert.deepEqual(parsePlacements([{ ...placement, node }]), []);
});

test("placements are picked by slot", () => {
  assert.equal(placementsFor(parsePlacements([placement]), "composer").length, 1);
});

test("the store reads the first answer and every change", async () => {
  let changed: (event: { payload: unknown }) => void = () => {};
  const listened: string[] = [];
  await connectPlugins({
    invoke: async cmd => { assert.equal(cmd, "plugins_placements"); return [placement]; },
    listen: async (event, handler) => { listened.push(event); changed = handler; return () => {}; },
  });
  assert.deepEqual(listened, ["plugins-changed"]);
  assert.equal(pluginsStore.getState().placements.length, 1);

  changed({ payload: [] });
  assert.equal(pluginsStore.getState().placements.length, 0);
});

const agent = (key: string | null, machine: string | null = null): WantedTopic => ({ name: "agent", key, machine });

test("wanted topics are read and invalid ones dropped", () => {
  const good = [agent("s1", "build-box"), agent(null), { name: "ui.composer", key: null, machine: null }];
  const bad = [
    { name: "agent" },
    { name: "agent", key: "", machine: null },
    { name: "agent", key: 3, machine: null },
    { name: "agent", key: null, machine: "a b" },
    { name: "bad name", key: null, machine: null },
    "agent",
  ];
  assert.deepEqual(parseWanted([...bad, ...good]), good);
  for (const payload of [null, {}, "x", undefined]) assert.deepEqual(parseWanted(payload), []);
});

test("only session topics go to the mesh", () => {
  const wanted = [agent("s1"), { name: "agents", key: null, machine: null }, { name: "ui.composer", key: null, machine: null }, { name: "machines", key: null, machine: null }];
  assert.deepEqual(meshTopics(wanted).map(topic => topic.name), ["agent", "agents"]);
});

interface Heard { topic: WantedTopic; receive: (machine: string, data: unknown) => void; stopped: boolean }

function fakeBridge(first: unknown) {
  const heard: Heard[] = [];
  const calls: { cmd: string; args: Record<string, unknown> | undefined }[] = [];
  let wanted: (event: { payload: unknown }) => void = () => {};
  const bridge = {
    invoke: async (cmd: string, args?: Record<string, unknown>) => { calls.push({ cmd, args }); return cmd === "plugins_wanted" ? first : null; },
    listen: async (event: string, handler: (event: { payload: unknown }) => void) => { if (event === "plugins-wanted") wanted = handler; return () => {}; },
    follow: (topic: WantedTopic, receive: (machine: string, data: unknown) => void) => {
      const item: Heard = { topic, receive, stopped: false };
      heard.push(item);
      return () => { item.stopped = true; };
    },
  };
  return { bridge, heard, calls, change: (payload: unknown) => wanted({ payload }) };
}

// Runs first of the tests that connect: the page-wide publisher is set by the first connect.
test("ui updates are published, and the latest waits for the bridge", async () => {
  publishUi({ name: "ui.composer", data: { machine: "m1", session: "s0" } });
  publishUi({ name: "ui.composer", data: { machine: "m1", session: "s1" } });
  const fake = fakeBridge([]);
  await connectPluginTopics(fake.bridge);
  publishUi({ name: "ui.composer", data: null });

  const ui = { name: "ui.composer", key: null, machine: null };
  const sent = fake.calls.filter(call => call.cmd === "plugins_publish").map(call => call.args);
  assert.deepEqual(sent, [
    { update: { topic: ui, machine: null, data: { machine: "m1", session: "s1" } } },
    { update: { topic: ui, machine: null, data: null } },
  ]);
});

test("the bridge follows new topics and drops removed ones", async () => {
  const fake = fakeBridge([agent("s1", "m1")]);
  await connectPluginTopics(fake.bridge);
  assert.deepEqual(fake.heard.map(item => item.topic), [agent("s1", "m1")]);

  fake.change([agent("s1", "m1"), agent("s2", "m1"), { name: "ui.composer", key: null, machine: null }]);
  assert.deepEqual(fake.heard.map(item => item.topic), [agent("s1", "m1"), agent("s2", "m1")]);
  assert.equal(fake.heard[0].stopped, false);

  fake.change([agent("s2", "m1")]);
  assert.deepEqual(fake.heard.map(item => item.stopped), [true, false]);

  fake.change([]);
  assert.equal(fake.heard[1].stopped, true);
  assert.equal(fake.heard.length, 2);
});

test("an event that arrives before the first read wins", async () => {
  let release: () => void = () => {};
  const fake = fakeBridge([agent("old")]);
  const slow = { ...fake.bridge, invoke: (cmd: string, args?: Record<string, unknown>) => cmd === "plugins_wanted" ? new Promise<unknown>(resolve => { release = () => resolve([agent("old")]); }) : fake.bridge.invoke(cmd, args) };
  const done = connectPluginTopics(slow);
  await new Promise(resolve => setTimeout(resolve, 0));
  fake.change([agent("new")]);
  release();
  await done;
  assert.deepEqual(fake.heard.map(item => item.topic.key), ["new"]);
});

test("updates are forwarded with the topic and the sending machine", async () => {
  const fake = fakeBridge([agent("s1")]);
  await connectPluginTopics(fake.bridge);

  fake.heard[0].receive("m9", { x: 1 });

  const sent = fake.calls.filter(call => call.cmd === "plugins_publish");
  assert.deepEqual(sent.map(call => call.args), [{ update: { topic: agent("s1"), machine: "m9", data: { x: 1 } } }]);
});
