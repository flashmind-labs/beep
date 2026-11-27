import { answers } from "./steer-env.ts";
import test from "node:test";
import assert from "node:assert/strict";
import { ensureMachines, MACHINES, machinesChanged, REMOVED, removeListed, renameListed, takeTraffic } from "../src/stores/machines/machine-store.ts";
import type { Machine } from "../src/screens/machines/machines.tsx";

const held = (): Machine[] | null => MACHINES.list;
const named = (name: string) => ({ id: "m1", name }) as unknown as Machine;

test("the first read fills the machines and later calls use the held list", async () => {
  MACHINES.list = null;
  answers.list_machines = [named("first")];
  assert.equal((await ensureMachines())[0]?.name, "first");

  answers.list_machines = [named("changed")];
  assert.equal((await ensureMachines())[0]?.name, "first");
});

test("a list set while the first read is out wins over its answer", async () => {
  MACHINES.list = null;
  let answer!: (list: Machine[]) => void;
  answers.list_machines = new Promise<Machine[]>(resolve => { answer = resolve; });
  const reading = ensureMachines();

  MACHINES.list = [named("renamed")];
  answer([named("old")]);
  await reading;

  assert.equal(held()?.[0]?.name, "renamed");
});

test("a rename or removal changes the one list and tells its readers", () => {
  const heard: string[][] = [];
  const hear = () => heard.push((MACHINES.list ?? []).map(m => m.name));
  machinesChanged.add(hear);

  try {
    MACHINES.list = [named("one"), { ...named("two"), id: "m2" }];
    renameListed("m1", "first");
    removeListed("m2");
    MACHINES.list = null;
    renameListed("m1", "ignored");
  } finally {
    machinesChanged.delete(hear);
  }

  assert.deepEqual(heard, [["first", "two"], ["first"]]);
});

test("a removal made here is remembered, and told even before the list arrived", () => {
  let heard = 0;
  const hear = () => { heard++; };
  machinesChanged.add(hear);

  try {
    MACHINES.list = null;
    REMOVED.clear();
    removeListed("m9");
  } finally {
    machinesChanged.delete(hear);
  }

  assert.equal(heard, 1);
  assert.ok(REMOVED.has("m9"));
  REMOVED.clear();
});

test("traffic is taken from a telemetry update and only when it changed", () => {
  const traffic = [{ id: "m1", counters: [{ key: "endpoint.relay.rx", bytes: 5 }] }];
  MACHINES.traffic = [];

  assert.equal(takeTraffic({ stats: [], traffic }), true);
  assert.deepEqual(MACHINES.traffic, traffic);
  assert.equal(takeTraffic({ stats: [], traffic: structuredClone(traffic) }), false);
  assert.equal(takeTraffic({ stats: [] }), false);
  assert.equal(takeTraffic(null), false);
});
