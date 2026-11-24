import { calls, nativeEvent } from "./bar-env.ts";
import test from "node:test";
import assert from "node:assert/strict";
import { MACHINES } from "../src/machines/machine-store.ts";
import type { MeshEvent } from "../src/ui/mesh-events.ts";

const settle = () => new Promise<void>(resolve => setImmediate(resolve));
type Reader = { onEvent: { onmessage(event: MeshEvent): void } };

test("the machines push carries this computer and the machines that update, so no read asks for them", async () => {
  nativeEvent("center-shown");
  await settle();
  const reader = calls.filter(call => call.cmd === "mesh_watch").at(-1)!.args as Reader;
  const views = [
    { command: "list_machines", args: {}, value: [] },
    { command: "mac_control", args: {}, value: { id: "mac-1" } },
    { command: "updatable_machines", args: {}, value: [{ id: "box-1", agent_sessions: 0 }] },
  ];
  reader.onEvent.onmessage({ revision: 1, machine_id: "me", topic: "machines.*", data: { views } });

  assert.equal(MACHINES.mac, "mac-1");
  assert.deepEqual([...MACHINES.updatable], ["box-1"]);
  assert.equal(calls.some(call => call.cmd === "updatable_machines" || call.cmd === "mac_control"), false);
});
