import { answers } from "./steer-env.ts";
import test from "node:test";
import assert from "node:assert/strict";
import { loadMachines, MACHINES, setMachines } from "../src/machines/machine-store.ts";
import type { Machine } from "../src/machines/machines.tsx";

const named = (name: string) => ({ id: "m1", name }) as unknown as Machine;

test("a read that started before a local edit does not undo it", async () => {
  let answer!: (list: Machine[]) => void;
  answers.list_machines = new Promise<Machine[]>(resolve => { answer = resolve; });
  const reading = loadMachines();

  setMachines([named("renamed")]);
  answer([named("old")]);
  await reading;

  assert.equal(MACHINES.list?.[0]?.name, "renamed");
});
