import test from "node:test";
import assert from "node:assert/strict";
import { clipBytes, connectPluginSlots, connectPluginTopics, connectPlugins, createInstanceBook, indexPlacements, instanceFits, instanceId, meshTopics, parsePlacements, parseWanted, placementsFor, pluginsStore, publishUi, sendPluginEvent } from "../src/plugins/plugins-state.ts";
import type { InstanceBatch, SlotInstance, WantedTopic } from "../src/plugins/plugins-state.ts";

const image = { kind: "image", src: "data:image/png;base64,AAAA", width: 60, height: 48, pixelated: true };
const placement = { plugin: "composer-cat", slot: "composer", instance: "composer", position: { kind: "edge", align: "end", offset: 84, sink: 3 }, node: image };
const text = (value: string, tone = "normal") => ({ kind: "text", text: value, tone });
const inline = (slot: string, instance: string, node: unknown) => ({ plugin: "session-notes", slot, instance, position: { kind: "inline" }, node });

test("a valid placement is read as sent", () => {
  assert.deepEqual(parsePlacements([placement]), [placement]);
});

test("text and nested rows are read", () => {
  const node = { kind: "row", gap: 4, children: [text("hi", "accent"), image] };
  assert.deepEqual(parsePlacements([{ ...placement, node }]), [{ ...placement, node }]);
});

test("a payload that is not a list is empty", () => {
  for (const payload of [null, {}, "x", 3, undefined]) assert.deepEqual(parsePlacements(payload), []);
});

test("invalid placements are dropped and the rest kept", () => {
  const bad = [
    { ...placement, slot: "sidebar" },
    { ...placement, position: { ...placement.position, align: "middle" } },
    { ...placement, position: { ...placement.position, offset: -1 } },
    { ...placement, position: { ...placement.position, sink: 1.5 } },
    { ...placement, position: { kind: "inline" } },
    { ...placement, instance: "" },
    { ...placement, position: null },
    { ...placement, plugin: 4 },
    { ...placement, node: { ...image, src: "https://example.com/cat.png" } },
    { ...placement, node: { ...image, src: "javascript:alert(1)" } },
    { ...placement, node: { ...image, width: "60" } },
    { ...placement, node: { ...image, pixelated: 1 } },
    { ...placement, node: { kind: "script" } },
    { ...placement, node: { kind: "row", gap: 2, children: [{ kind: "text" }] } },
    { ...placement, node: { kind: "text", text: "x", tone: "loud" } },
    { ...inline("session.row", "a", text("x")), position: { kind: "edge", align: "end", offset: 0, sink: 0 } },
    null,
    "placement",
  ];
  assert.deepEqual(parsePlacements([...bad, placement]), [placement]);
});

test("rows nested too deep are refused", () => {
  let node: unknown = text("deep");
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

const button = { kind: "button", label: "Save", children: [text("Save")], onPress: 7 };
const toggle = { kind: "toggle", label: "On", value: true, onChange: 8 };
const input = { kind: "input", label: "Note", value: "a", placeholder: "Add", onChange: 9, onSubmit: null };

test("inline placements and interactive nodes are read", () => {
  const list = [inline("session.row", "m:a", text("note", "accent")), inline("session.header", "m:a", { kind: "row", gap: 6, children: [input, button, toggle] })];
  assert.deepEqual(parsePlacements(list), list);
});

test("controls without valid callbacks are dropped", () => {
  const bad = [
    { ...button, onPress: 0 }, { ...button, onPress: "7" }, { ...button, label: 1 }, { ...button, children: "x" },
    { ...toggle, value: "yes" }, { ...toggle, onChange: null },
    { ...input, placeholder: null }, { ...input, onChange: "9" }, { ...input, onSubmit: 1.5 },
  ];
  assert.deepEqual(parsePlacements(bad.map(node => inline("session.header", "k", node))), []);
});

test("placements are indexed by instance and keep their list when unchanged", () => {
  const rows = parsePlacements([inline("session.row", "a", text("A")), inline("session.row", "b", text("B")), inline("session.header", "a", text("H"))]);
  const first = indexPlacements(rows, new Map());
  assert.deepEqual([...first.keys()], [instanceId("session.row", "a"), instanceId("session.row", "b"), instanceId("session.header", "a")]);

  const next = indexPlacements(parsePlacements([inline("session.row", "a", text("A")), inline("session.row", "b", text("B2"))]), first);
  assert.equal(next.get(instanceId("session.row", "a")), first.get(instanceId("session.row", "a")));
  assert.notEqual(next.get(instanceId("session.row", "b")), first.get(instanceId("session.row", "b")));
  assert.equal(next.has(instanceId("session.header", "a")), false);
});

test("the store indexes what the host sends", async () => {
  await connectPlugins({ invoke: async () => [inline("session.row", "m:a", text("x"))], listen: async () => () => {} });
  assert.equal(pluginsStore.getState().index.get(instanceId("session.row", "m:a"))?.length, 1);
});

const row = (session: string, title = "T"): SlotInstance => ({ slot: "session.row", key: `m:${session}`, context: { machine: "m", session, title } });
const header = (session: string): SlotInstance => ({ slot: "session.header", key: `m:${session}`, context: { machine: "m", session } });

/// A book whose microtasks run when the test says.
function book() {
  const queue: (() => void)[] = [];
  const sent: InstanceBatch[] = [];
  const it = createInstanceBook(run => queue.push(run));
  return { it, sent, connect: () => it.connect(batch => sent.push(batch)), tick: () => { while (queue.length) queue.shift()?.(); } };
}

test("instances shown before the bridge connects go in its first batch, which resets", () => {
  const b = book();
  b.it.show(row("a"));
  b.it.show(row("b"));
  b.it.hide("session.row", "m:b");
  b.tick();
  assert.deepEqual(b.sent, []);

  b.connect();
  b.tick();
  assert.deepEqual(b.sent, [{ reset: true, shown: [row("a")], gone: [{ slot: "session.row", key: "m:b" }] }]);
});

test("a connect with nothing shown still resets the host", () => {
  const b = book();
  b.connect();
  b.tick();
  assert.deepEqual(b.sent, [{ reset: true, shown: [], gone: [] }]);
});

test("changes in one turn make one batch, and an unchanged instance sends nothing", () => {
  const b = book();
  b.connect();
  b.tick();
  b.sent.length = 0;

  for (let i = 0; i < 50; i++) b.it.show(row(`s${i}`));
  b.it.show(header("s0"));
  b.tick();
  assert.equal(b.sent.length, 1);
  assert.equal(b.sent[0].shown.length, 51);
  assert.equal(b.sent[0].reset, false);

  // Mounting the same instance again with the same context tells the host nothing.
  b.it.show(row("s0"));
  b.tick();
  assert.equal(b.sent.length, 1);
  b.it.hide("session.row", "m:s0");
  b.tick();
  assert.equal(b.sent.length, 1);
});

test("rows that scroll out report as gone, and a changed context reports alone", () => {
  const b = book();
  b.connect();
  b.it.show(row("a"));
  b.it.show(row("b"));
  b.tick();
  b.sent.length = 0;

  b.it.hide("session.row", "m:a");
  b.it.hide("session.row", "m:b");
  b.it.show(row("b", "Renamed"));
  b.tick();

  assert.deepEqual(b.sent, [{ reset: false, shown: [row("b", "Renamed")], gone: [{ slot: "session.row", key: "m:a" }] }]);
});

test("a slot reports at most two hundred instances, and a key is held until every view hides it", () => {
  const b = book();
  b.connect();
  const shown = Array.from({ length: 205 }, (_, i) => b.it.show(row(`s${i}`)));
  assert.equal(shown.filter(Boolean).length, 200);
  assert.equal(b.it.show(header("h")), true, "another slot has its own room");

  b.it.show(row("s0"));
  b.tick();
  b.sent.length = 0;
  b.it.hide("session.row", "m:s0");
  b.tick();
  assert.deepEqual(b.sent, []);
  b.it.hide("session.row", "m:s0");
  b.tick();
  assert.deepEqual(b.sent, [{ reset: false, shown: [], gone: [{ slot: "session.row", key: "m:s0" }] }]);
});

test("an instance the host would refuse is not reported", () => {
  const b = book();
  b.connect();

  assert.equal(b.it.show({ ...row("a"), key: "" }), false);
  assert.equal(b.it.show({ ...row("a"), key: "a".repeat(129) }), false);
  assert.equal(b.it.show({ ...row("a"), key: "a\nb" }), false);
  assert.equal(b.it.show(row("a", "t".repeat(513))), false);
  assert.equal(instanceFits({ slot: "session.row", key: "k", context: { machine: "m".repeat(400), session: "s".repeat(400), title: "t".repeat(400) } }), false);
  b.tick();
  assert.deepEqual(b.sent.at(-1)?.shown, []);
});

test("titles are cut to a byte limit at a character", () => {
  assert.equal(clipBytes("abc", 5), "abc");
  assert.equal(clipBytes("abcdef", 4), "abcd");
  assert.equal(clipBytes("ééé", 5), "éé");
  assert.equal(new TextEncoder().encode(clipBytes("😀".repeat(100), 256)).length, 256);
});

test("controls send their events through plugins_event once connected", async () => {
  const calls: { cmd: string; args: Record<string, unknown> }[] = [];
  sendPluginEvent({ plugin: "p-one", id: 1, action: { kind: "press" } });
  connectPluginSlots({ invoke: async (cmd, args) => { calls.push({ cmd, args }); return null; } });
  sendPluginEvent({ plugin: "session-notes", id: 7, action: { kind: "text", value: "hi" } });
  await new Promise(resolve => setTimeout(resolve, 0));

  const events = calls.filter(call => call.cmd === "plugins_event");
  assert.deepEqual(events.map(call => call.args), [{ event: { plugin: "session-notes", id: 7, action: { kind: "text", value: "hi" } } }]);
  const batches = calls.filter(call => call.cmd === "plugins_instances");
  assert.equal(batches.length, 1);
  assert.equal((batches[0].args.batch as InstanceBatch).reset, true);
});
