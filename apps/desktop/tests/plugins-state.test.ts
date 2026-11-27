import test from "node:test";
import assert from "node:assert/strict";
import { assetRefs, publishTurn, sessionTurn, clipBytes, connectPluginSlots, connectPluginTopics, connectPluginVisibility, connectPlugins, createInstanceBook, followPlugin, indexPlacements, instanceFits, instanceId, meshTopics, parsePlacements, parseWanted, placementsFor, pluginsStore, publishUi, sendPluginEvent, startDraft, typeInto } from "../src/stores/plugins/plugins-state.ts";
import type { InputDraft, InstanceBatch, SlotInstance, WantedTopic } from "../src/stores/plugins/plugins-state.ts";

const ref = (digit: string, mime = "image/png") => ({ hash: digit.repeat(64), mime });
const image = { kind: "image", asset: ref("a"), width: 60, height: 48, pixelated: true };
const sprite = { kind: "sprite", frames: [ref("a"), ref("b"), ref("c")], rest: 0, width: 72, height: 56, pixelated: true, motions: [
  { steps: [{ frame: 1, ms: 140 }], repeat: { kind: "every", minMs: 3000, maxMs: 7000 } },
  { steps: [{ frame: 2, ms: 200 }, { frame: 1, ms: 200 }], repeat: { kind: "once" } },
], reactions: { happy: [{ frame: 2, ms: 300 }], yawn: [{ frame: 1, ms: 100 }, { frame: 2, ms: 100 }] }, play: { name: "happy", id: 3 } };
const placement = { plugin: "composer-cat", slot: "composer", instance: "composer", position: { kind: "edge", align: "end", offset: 84, sink: 3 }, enter: "none", leave: "none", node: image };
const text = (value: string, tone = "normal") => ({ kind: "text", text: value, tone });
const inline = (slot: string, instance: string, node: unknown) => ({ plugin: "session-notes", slot, instance, position: { kind: "inline" }, enter: "none", leave: "none", node });

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
    { ...placement, node: { ...image, asset: "https://example.com/cat.png" } },
    { ...placement, node: { ...image, asset: ref("A") } },
    { ...placement, node: { ...image, asset: ref("a", "text/html") } },
    { ...placement, node: { ...image, asset: { hash: "abc", mime: "image/png" } } },
    { ...placement, node: { ...image, asset: null } },
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

test("enter and leave are read, and a value that is not a transition drops the placement", () => {
  for (const enter of ["none", "fade", "rise"]) for (const leave of ["none", "fade", "rise"]) {
    assert.deepEqual(parsePlacements([{ ...placement, enter, leave }]), [{ ...placement, enter, leave }]);
  }
  for (const bad of ["slide", null, 1, undefined]) {
    assert.deepEqual(parsePlacements([{ ...placement, enter: bad }, { ...placement, leave: bad }]), []);
  }
});

test("rows nested too deep are refused", () => {
  let node: unknown = text("deep");
  for (let i = 0; i < 12; i++) node = { kind: "row", gap: 0, children: [node] };
  assert.deepEqual(parsePlacements([{ ...placement, node }]), []);
});

test("placements are picked by slot", () => {
  assert.equal(placementsFor(parsePlacements([placement]), "composer").length, 1);
});

test("sprites are read as sent", () => {
  assert.deepEqual(parsePlacements([{ ...placement, node: sprite }]), [{ ...placement, node: sprite }]);
  assert.deepEqual(parsePlacements([{ ...placement, node: { ...sprite, motions: [] } }]).length, 1);
  assert.deepEqual(parsePlacements([{ ...placement, node: { ...sprite, reactions: {}, play: null } }]).length, 1);
  const loop = { ...sprite, motions: [{ steps: [{ frame: 1, ms: 80 }, { frame: 2, ms: 80 }], repeat: { kind: "loop" } }] };
  assert.deepEqual(parsePlacements([{ ...placement, node: loop }]), [{ ...placement, node: loop }]);
});

test("invalid sprites are dropped", () => {
  const motion = (extra: object) => ({ steps: [{ frame: 1, ms: 100 }], repeat: { kind: "once" }, ...extra });
  const steps = (count: number) => Array.from({ length: count }, () => ({ frame: 1, ms: 100 }));
  const bad = [
    { ...sprite, frames: [] },
    { ...sprite, frames: Array.from({ length: 33 }, () => ref("a")) },
    { ...sprite, frames: [ref("a"), "b"] },
    { ...sprite, rest: 3 },
    { ...sprite, rest: -1 },
    { ...sprite, rest: 0.5 },
    { ...sprite, width: -1 },
    { ...sprite, pixelated: "yes" },
    { ...sprite, motions: "none" },
    { ...sprite, motions: Array.from({ length: 9 }, () => motion({})) },
    { ...sprite, motions: [motion({ steps: [] })] },
    { ...sprite, motions: [motion({ steps: steps(65) })] },
    { ...sprite, motions: [motion({ steps: [{ frame: 3, ms: 100 }] })] },
    { ...sprite, motions: [motion({ steps: [{ frame: 1, ms: 15 }] })] },
    { ...sprite, motions: [motion({ steps: [{ frame: 1, ms: 60001 }] })] },
    { ...sprite, motions: [motion({ repeat: { kind: "every", minMs: 99, maxMs: 500 } })] },
    { ...sprite, motions: [motion({ repeat: { kind: "every", minMs: 600, maxMs: 500 } })] },
    { ...sprite, motions: [motion({ repeat: { kind: "every", minMs: 500, maxMs: 3600001 } })] },
    { ...sprite, motions: [motion({ repeat: { kind: "sometimes" } })] },
    { ...sprite, motions: [motion({ repeat: { kind: "loop" } }), motion({})] },
    { ...sprite, reactions: undefined },
    { ...sprite, reactions: { happy: [] } },
    { ...sprite, reactions: { happy: [{ frame: 3, ms: 100 }] } },
    { ...sprite, reactions: { happy: [{ frame: 1, ms: 15 }] } },
    { ...sprite, reactions: Object.fromEntries(Array.from({ length: 9 }, (_, index) => [`r${index}`, [{ frame: 1, ms: 100 }]])), play: null },
    { ...sprite, play: { name: "sad", id: 1 } },
    { ...sprite, play: { name: "constructor", id: 1 } },
    { ...sprite, play: { name: "happy", id: -1 } },
    { ...sprite, play: { name: "happy", id: 1.5 } },
    { ...sprite, play: undefined },
  ];
  for (const node of bad) assert.deepEqual(parsePlacements([{ ...placement, node }]), [], JSON.stringify(node).slice(0, 120));
});

test("every image a placement uses is listed, frames included", () => {
  const row = { kind: "row", gap: 0, children: [image, sprite] };
  const refs = assetRefs(parsePlacements([{ ...placement, node: row }]));
  assert.deepEqual(refs.map(item => item.hash[0]), ["a", "a", "b", "c"]);
});

test("the store reads the first answer and every change", async () => {
  let changed: (event: { payload: unknown }) => void = () => {};
  const listened: string[] = [];
  const backend = { load: async () => new ArrayBuffer(1), create: () => "blob:x", revoke: () => {} };
  await connectPlugins({
    invoke: async cmd => { assert.equal(cmd, "plugins_placements"); return [placement]; },
    listen: async (event, handler) => { listened.push(event); changed = handler; return () => {}; },
  }, backend);
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

test("the page tells the desktop whether it is visible, now and on each change", () => {
  const calls: unknown[] = [];
  let change: (shown: boolean) => void = () => {};
  connectPluginVisibility({
    invoke: async (cmd, args) => { calls.push([cmd, args]); return null; },
    shown: () => true,
    onChange: fn => { change = fn; },
  });
  change(false);
  change(true);

  assert.deepEqual(calls, [["plugins_visible", { visible: true }], ["plugins_visible", { visible: false }], ["plugins_visible", { visible: true }]]);
});

test("a late echo of older text does not replace newer typing", () => {
  let draft: InputDraft = startDraft("");
  for (const typed of ["a", "ab", "abc"]) draft = typeInto(draft, typed);

  // The plugin answers "a" and "ab" after the user typed "abc".
  draft = followPlugin(draft, "a");
  assert.equal(draft.text, "abc");
  draft = followPlugin(draft, "ab");
  assert.equal(draft.text, "abc");
  draft = followPlugin(draft, "abc");
  assert.deepEqual(draft, { text: "abc", pending: [] });
});

test("echoes that skip a value or arrive out of step keep the newest typing", () => {
  let draft: InputDraft = startDraft("");
  for (const typed of ["a", "ab", "abc"]) draft = typeInto(draft, typed);

  // "ab" arrives first, which also settles "a".
  draft = followPlugin(draft, "ab");
  assert.deepEqual(draft, { text: "abc", pending: ["abc"] });

  // The user types on before the last echo.
  draft = typeInto(draft, "abcd");
  draft = followPlugin(draft, "abc");
  assert.deepEqual(draft, { text: "abcd", pending: ["abcd"] });
  draft = followPlugin(draft, "abcd");
  assert.deepEqual(draft, { text: "abcd", pending: [] });
});

test("a value the plugin sets on its own replaces the draft", () => {
  let draft: InputDraft = startDraft("");
  draft = typeInto(draft, "hello");
  draft = followPlugin(draft, "hello");

  // The plugin clears the field after a submit.
  draft = followPlugin(draft, "");
  assert.deepEqual(draft, { text: "", pending: [] });

  // Typing then works as before.
  draft = typeInto(draft, "x");
  assert.deepEqual(followPlugin(draft, "x"), { text: "x", pending: [] });
});

test("a plugin value that was never sent replaces the draft and forgets older sends", () => {
  let draft: InputDraft = startDraft("start");
  draft = typeInto(draft, "start!");

  draft = followPlugin(draft, "reset by plugin");
  assert.deepEqual(draft, { text: "reset by plugin", pending: [] });
  // The echo of the older send is now a plugin value too, since the page no longer waits for it.
  assert.deepEqual(followPlugin(draft, "start!"), { text: "start!", pending: [] });
});

test("the page keeps a bounded list of values it waits for", () => {
  let draft: InputDraft = startDraft("");
  for (let index = 0; index < 200; index++) draft = typeInto(draft, "x".repeat(index + 1));

  assert.equal(draft.pending.length, 64);
  assert.equal(draft.text.length, 200);
});

const page = (status: string, turns: number, last_stop: string | null, extra: object = {}) => ({ session: { id: "s1", status, ...extra }, turns, last_stop });

test("session.turn maps the page's status and the node's stop reason", () => {
  const states: [string, object, string][] = [
    ["starting", {}, "starting"], ["working", {}, "working"], ["running", {}, "working"], ["asking", {}, "asking"], ["idle", {}, "idle"],
    ["ended", {}, "ended"], ["waiting", {}, "idle"], ["idle", { subagents: 2 }, "working"], ["idle", { subagents: 0 }, "idle"],
    ["working", { question: { id: 1 } }, "asking"], ["idle", { question: null }, "idle"],
  ];
  for (const [status, extra, state] of states) assert.equal(sessionTurn(page(status, 2, null, extra)).state, state, `${status} ${JSON.stringify(extra)}`);

  const outcomes: [string | null | undefined, string | null][] = [
    ["EndTurn", "finished"], ["Cancelled", "cancelled"], ["Failed", "failed"], ["Refusal", "refused"], ["MaxTokens", "token-limit"],
    ["MaxTurnRequests", "request-limit"], ["Other", "stopped"], ["FromANewerNode", "stopped"], [null, null], [undefined, null],
  ];
  for (const [stop, outcome] of outcomes) assert.deepEqual(sessionTurn({ ...page("idle", 4, null), last_stop: stop }), { session: "s1", state: "idle", outcome, turn: 4 }, String(stop));
});

test("session.turn reaches plugins that follow it, once for each change, and a late follower gets the latest", async () => {
  const turn = { name: "session.turn", key: "s1", machine: "m1" };
  const fake = fakeBridge([turn, { name: "session.turn", key: "other", machine: "m1" }]);
  publishTurn("m1", page("working", 3, "EndTurn"));
  await connectPluginTopics(fake.bridge);
  const sent = () => fake.calls.filter(call => call.cmd === "plugins_publish").map(call => call.args);
  const data = (state: string, outcome: string | null, count: number) => ({ update: { topic: turn, machine: "m1", data: { session: "s1", state, outcome, turn: count } } });

  // What the page knew before the plugin followed arrives at once, and only for the session it named.
  assert.deepEqual(sent(), [data("working", "finished", 3)]);

  // The same snapshot again is not a new update; a new end is.
  publishTurn("m1", page("working", 3, "EndTurn"));
  publishTurn("m1", page("idle", 4, "Cancelled"));
  publishTurn("m1", page("idle", 4, "Cancelled"));
  publishTurn("m2", page("idle", 9, "Failed"));
  publishTurn("m1", { ...page("idle", 4, "Cancelled"), session: { id: "s2", status: "idle" } });
  assert.deepEqual(sent(), [data("working", "finished", 3), data("idle", "cancelled", 4)]);

  // Followed with no key or machine, every session's updates arrive.
  fake.change([{ name: "session.turn", key: null, machine: null }]);
  assert.equal(sent().length, 2 + 3);
  publishTurn("m1", page("working", 4, "Cancelled"));
  assert.equal(sent().length, 2 + 3 + 1);
});
