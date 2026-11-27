import { calls, nativeEvent } from "./bar-env.ts";
import test from "node:test";
import assert from "node:assert/strict";
import type { MeshEvent } from "../src/ui/mesh-events.ts";
import { TopicValues } from "../src/ui/topic-values-state.ts";
import { parseProcesses } from "../src/stores/agentpane/running-state.ts";

const settle = () => new Promise<void>(resolve => setImmediate(resolve));
type Reader = { onEvent: { onmessage(event: MeshEvent): void } };

const number = (data: unknown) => typeof data === "number" ? data : null;
let revision = 0;
const push = (reader: Reader, data: unknown, topic = "count.one", machine = "machine-a", query?: number) =>
  reader.onEvent.onmessage({ revision: ++revision, machine_id: machine, topic, data, query });
const reader = () => calls.filter(call => call.cmd === "mesh_watch").at(-1)?.args as Reader;

test("a view gets the kept value at once, then each update, and unreadable data keeps the last value", async () => {
  nativeEvent("center-shown");
  const values = new TopicValues(number, 4);
  const seen: number[] = [];
  const stop = values.watch("machine-a", "count.one", value => seen.push(value));
  await settle();
  push(reader(), 1);
  push(reader(), "bad");
  push(reader(), 2);
  stop();
  assert.deepEqual(seen, [1, 2]);
  assert.equal(values.get("machine-a", "count.one"), 2);

  const again: number[] = [];
  const stopAgain = values.watch("machine-a", "count.one", value => again.push(value));
  assert.deepEqual(again, [2]);
  stopAgain();
  await settle();
});

test("other machines, other topics and query answers are not this value", async () => {
  const values = new TopicValues(number, 4);
  const seen: number[] = [];
  const stop = values.watch("machine-a", "count.one", value => seen.push(value));
  await settle();
  push(reader(), 1, "count.one", "machine-b");
  push(reader(), 2, "count.two");
  push(reader(), 3, "count.one", "machine-a", 7);
  push(reader(), 4);
  stop();
  assert.deepEqual(seen, [4]);
  assert.equal(values.get("machine-b", "count.one"), null);
  await settle();
});

test("the least recently used value goes past the limit", async () => {
  const values = new TopicValues(number, 2);
  const stops = ["one", "two", "three"].map(name => values.watch("machine-a", `count.${name}`, () => {}));
  await settle();
  push(reader(), 1, "count.one");
  push(reader(), 2, "count.two");
  assert.equal(values.get("machine-a", "count.one"), 1);
  push(reader(), 3, "count.three");
  assert.equal(values.get("machine-a", "count.two"), null);
  assert.equal(values.get("machine-a", "count.one"), 1);
  assert.equal(values.get("machine-a", "count.three"), 3);
  stops.forEach(stop => stop());
  await settle();
});

test("a change the owner makes replaces the kept value", () => {
  const values = new TopicValues(number, 2);
  values.put("machine-a", "count.one", 5);
  assert.equal(values.get("machine-a", "count.one"), 5);
});

test("a process list is read from the node's value", () => {
  const process = { pid: 41, command: "npm run dev", started_at: 1, ports: [3000], left_group: false, parent: 0, pages: [] };
  assert.deepEqual(parseProcesses({ Sessions: { Processes: { processes: [process] } } }), { kind: "list", processes: [process], containers: [] });
  assert.deepEqual(parseProcesses("Missing"), { kind: "list", processes: [], containers: [] });
  const container = { id: "c0ffee", name: "web", image: "nginx", started_at: 1, ports: [8080], pages: [8080] };
  assert.deepEqual(parseProcesses({ Sessions: { Processes: { processes: [], containers: [container] } } }), { kind: "list", processes: [], containers: [container] });
  assert.equal(parseProcesses({ Sessions: { Processes: { processes: [], containers: [{ id: 1 }] } } }), null);
  assert.deepEqual(parseProcesses({ Sessions: { Error: { message: "no such session" } } }), { kind: "failed", message: "no such session" });
  assert.deepEqual(parseProcesses({ error: "the node did not answer" }), { kind: "failed", message: "the node did not answer" });
  assert.equal(parseProcesses({ Sessions: { Processes: { processes: [{ pid: "41" }] } } }), null);
  assert.equal(parseProcesses({ Process: { List: {} } }), null);
  assert.equal(parseProcesses(null), null);
});
