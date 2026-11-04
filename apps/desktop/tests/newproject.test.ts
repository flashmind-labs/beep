import test from "node:test";
import assert from "node:assert/strict";
import { canCreate, landedLines, pickHost, startForm, toggleMachine, validName } from "../src/addproject/newproject-state.ts";
import type { Forge } from "../src/addproject/newproject-state.ts";

const forges: Forge[] = [
  { host: "git.example.com", github: false, owners: [], error: "git.example.com did not answer in time." },
  { host: "github.com", github: true, owners: ["octo", "acme"], error: null },
];

test("the form starts on the first host that answered, and on Local only without one", () => {
  const form = startForm(forges, "m1");
  assert.equal(form.host, "github.com");
  assert.equal(form.owner, "octo");
  assert.equal(startForm([], "m1").host, null);
});

test("Local only keeps one machine and hosts take several", () => {
  let form = startForm(forges, "m1");
  form = toggleMachine(form, "m2");
  assert.deepEqual(form.machines, ["m1", "m2"]);
  form = pickHost(form, forges, null);
  assert.deepEqual(form.machines, ["m1"]);
  assert.deepEqual(toggleMachine(form, "m2").machines, ["m2"]);
});

test("Create waits for a valid name and a machine", () => {
  const form = startForm(forges, "m1");
  assert.ok(!canCreate(form));
  assert.ok(canCreate({ ...form, name: "widget" }));
  assert.ok(!canCreate({ ...form, name: "widget", machines: [] }));
  for (const bad of ["-x", ".x", "a b", "a/b", "x.git", "x".repeat(101)]) assert.ok(!validName(bad), bad);
});

test("machines in one folder share a line", () => {
  const a = { machine_name: "a", folder: "/srv/w" }, b = { machine_name: "b", folder: "/srv/w" }, c = { machine_name: "c", folder: "/opt/w" };
  assert.deepEqual(landedLines([a, b]), ["On a and b in /srv/w"]);
  assert.deepEqual(landedLines([a, c]), ["On a in /srv/w", "On c in /opt/w"]);
});
