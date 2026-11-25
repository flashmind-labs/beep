import { calls, nativeEvent } from "./bar-env.ts";
import test from "node:test";
import assert from "node:assert/strict";
import { MACHINES, REMOVED, removeListed } from "../src/machines/machine-store.ts";
import type { MeshEvent } from "../src/ui/mesh-events.ts";

const settle = () => new Promise<void>(resolve => setImmediate(resolve));
type Reader = { onEvent: { onmessage(event: MeshEvent): void } };

test("the machines push carries this computer and the machines that update, so no read asks for them", async () => {
  nativeEvent("center-shown");
  await settle();
  const reader = calls.filter(call => call.cmd === "mesh_watch").at(-1)!.args as Reader;
  const views = [
    { command: "list_machines", args: {}, value: [] },
    { command: "mac_control", args: {}, value: { id: "mac-1", name: "Laptop", enabled: true, status: "connected", error: null, permissions: { screen_recording: true, accessibility: true } } },
    { command: "updatable_machines", args: {}, value: [{ id: "box-1", agent_sessions: 0 }] },
  ];
  reader.onEvent.onmessage({ revision: 1, machine_id: "me", topic: "machines.*", data: { views } });

  assert.equal(MACHINES.mac?.id, "mac-1");
  assert.equal(MACHINES.mac?.status, "connected");
  assert.deepEqual([...MACHINES.updatable], ["box-1"]);
  assert.equal(calls.some(call => call.cmd === "updatable_machines" || call.cmd === "mac_control"), false);
});

test("a machine whose removal lands after the push that dropped it shows when it is paired again", async () => {
  nativeEvent("center-shown");
  await settle();
  const reader = calls.filter(call => call.cmd === "mesh_watch").at(-1)!.args as Reader;
  const push = (revision: number, ids: string[]) => reader.onEvent.onmessage({
    revision, machine_id: "me", topic: "machines.*", data: { views: [{ command: "list_machines", args: {}, value: ids.map(id => ({ id, name: id })) }] },
  });
  REMOVED.clear();
  push(2, ["gone", "stay"]);

  push(3, ["stay"]);
  removeListed("gone");
  assert.equal(REMOVED.has("gone"), false);

  push(4, ["gone", "stay"]);
  assert.equal(REMOVED.has("gone"), false);
  assert.deepEqual(MACHINES.list?.map(m => m.id), ["gone", "stay"]);
});
