import { answers, bar, calls } from "./steer-env.ts";
import test from "node:test";
import assert from "node:assert/strict";
import { MC, machinesIn, machinesOut, renderMachines } from "../src/machines/machines.ts";
import { S } from "../src/bar/core.ts";

const settle = () => new Promise<void>(resolve => setImmediate(resolve));
answers.list_machines = [];
answers.updatable_machines = [];
answers.machine_traffic = [];
answers.machine_clocks = [];
answers.tool_updates = [];

test("machine screen releases detached controls and reopens with a new owner", async () => {
  S.view = "machines";
  await renderMachines();
  const mount = bar.firstElementChild;
  assert.ok(mount);
  assert.ok(mount.querySelector("#mcAdd"));
  S.view = "chat";
  bar.replaceChildren();
  await settle();
  assert.equal(mount.childElementCount, 0);

  S.view = "machines";
  await renderMachines();
  assert.notEqual(bar.firstElementChild, mount);
  assert.ok(bar.querySelector("#mcAdd"));
  S.view = "chat";
  bar.replaceChildren();
  await settle();
});

test("closing an embedded machine page releases its mounted controls", async () => {
  const host = document.createElement("div");
  document.body.append(host);
  machinesIn(host, () => {}, () => {});
  await renderMachines();
  const mount = host.firstElementChild;
  assert.ok(mount);
  assert.ok(mount.querySelector("#mcAdd"));
  machinesOut();
  assert.equal(mount.childElementCount, 0);
  host.remove();
});


test("reattaching the machine host before observer delivery retains its controls", async () => {
  const host = document.createElement("div");
  document.body.append(host);
  machinesIn(host, () => {}, () => {});
  await renderMachines();
  const mount = host.firstElementChild;
  assert.ok(mount);
  const button = mount.querySelector("#mcAdd");
  assert.ok(button);
  host.remove();
  document.body.append(host);
  await settle();
  assert.equal(host.firstElementChild, mount);
  assert.equal(mount.querySelector("#mcAdd"), button);
  machinesOut();
  host.remove();
});

test("overlapping machine refreshes wait for the latest read", async () => {
  let finish: (value: unknown) => void = () => {};
  const first = new Promise(resolve => { finish = resolve; });
  answers.list_machines = first;
  S.view = "machines";
  const before = calls.filter(call => call.cmd === "list_machines").length;
  const one = renderMachines();
  const two = renderMachines();
  await settle();
  assert.equal(calls.filter(call => call.cmd === "list_machines").length - before, 1);
  const newest = [{ id: "machine", name: "Current", online: true, seen_at: 0, version: "", env: null }];
  answers.list_machines = newest;
  finish([{ id: "machine", name: "Old", online: true, seen_at: 0, version: "", env: null }]);
  await Promise.all([one, two]);
  assert.equal(calls.filter(call => call.cmd === "list_machines").length - before, 2);
  assert.equal(MC.list?.[0]?.name, "Current");
  answers.list_machines = [];
  S.view = "chat";
  bar.replaceChildren();
  await settle();
});
