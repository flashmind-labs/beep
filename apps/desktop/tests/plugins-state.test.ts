import test from "node:test";
import assert from "node:assert/strict";
import { connectPlugins, parsePlacements, placementsFor, pluginsStore } from "../src/plugins/plugins-state.ts";

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
