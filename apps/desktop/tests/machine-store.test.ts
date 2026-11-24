import { answers } from "./steer-env.ts";
import test from "node:test";
import assert from "node:assert/strict";
import { ensureMachines, MACHINES, setMachines } from "../src/machines/machine-store.ts";
import type { Machine } from "../src/machines/machines.tsx";

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

  setMachines([named("renamed")]);
  answer([named("old")]);
  await reading;

  assert.equal(held()?.[0]?.name, "renamed");
});
