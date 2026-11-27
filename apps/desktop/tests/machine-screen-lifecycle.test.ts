import { answers, bar, calls } from "./steer-env.ts";
import test from "node:test";
import assert from "node:assert/strict";
import { MACHINES } from "../src/stores/machines/machine-store.ts";
import { machinesIn, machinesOut, renderMachines } from "../src/screens/machines/machines.tsx";
import { setView } from "../src/stores/bar/core.ts";
import "../src/shell/bar/bar-app.tsx";
import { openBarPage } from "../src/stores/bar/bar-page.ts";
import { flushSync } from "react-dom";

const settle = () => new Promise<void>(resolve => setImmediate(resolve));
answers.list_machines = [];
answers.machine_clocks = [];

/// Another page opens in the bar, as when the user leaves Machines.
function leave() {
  setView("waiting");
  flushSync(() => openBarPage(() => {}).show({ view: "none" }));
}

test("the bar's machine screen unmounts when another page opens and draws again when reopened", async () => {
  setView("machines");
  await renderMachines();
  const add = bar.querySelector("#mcAdd");
  assert.ok(add);
  leave();
  await settle();
  assert.equal(bar.querySelector("#mcAdd"), null);
  assert.equal(add.isConnected, false);

  setView("machines");
  await renderMachines();
  assert.ok(bar.querySelector("#mcAdd"));
  assert.notEqual(bar.querySelector("#mcAdd"), add);
  leave();
  await settle();
});

test("closing an embedded machine page releases its mounted controls", async () => {
  const host = document.createElement("div");
  document.body.append(host);
  machinesIn(host, () => {}, () => {});
  await renderMachines();
  assert.ok(host.querySelector("#mcAdd"));
  machinesOut();
  assert.equal(host.childElementCount, 0);
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

test("overlapping machine refreshes share one read", async () => {
  let finish: (value: unknown) => void = () => {};
  const first = new Promise(resolve => { finish = resolve; });
  answers.list_machines = first;
  Object.assign(MACHINES, { list: null });
  setView("machines");
  const before = calls.filter(call => call.cmd === "list_machines").length;
  const one = renderMachines();
  const two = renderMachines();
  await settle();
  assert.equal(calls.filter(call => call.cmd === "list_machines").length - before, 1);
  finish([{ id: "machine", name: "Current", online: true, seen_at: 0, version: "", env: null }]);
  await Promise.all([one, two]);
  assert.equal(calls.filter(call => call.cmd === "list_machines").length - before, 1);
  assert.equal(MACHINES.list?.[0]?.name, "Current");
  answers.list_machines = [];
  leave();
  await settle();
});
