import test from "node:test";
import assert from "node:assert/strict";
import { stateLabel, terminalTile, terminalPlaces, splice, fromBase64, retryDelay, linkLabel, byMachine, terminalWhere, terminalNote, terminalsCount, endsForGood } from "../src/terminals.ts";

const ago = () => "2 h";

test("a terminal's state reads plainly", () => {
  assert.equal(stateLabel({ state: "running" }), "running");
  assert.equal(stateLabel({ state: "exited", code: 0 }), "done");
  assert.equal(stateLabel({ state: "exited", code: 1 }), "exited 1");
  assert.equal(stateLabel({ state: "exited", code: null }), "ended");
  assert.equal(stateLabel({ state: "offline" }), "offline");
});

test("the Terminals tile counts running ones and notes the rest", () => {
  assert.equal(terminalTile(undefined, ago).empty, "Loading…");
  assert.equal(terminalTile(null, ago).empty, "The server keeps no terminals");
  const tile = terminalTile([
    { name: "dev-server", state: "running", started_at: 1, machine_name: "build-box", command: "npm run dev", cwd: "/srv/project" },
    { name: "logs", state: "exited", code: 1, started_at: 1, machine_name: "build-box", command: "tail -f x", cwd: "/srv/project" },
  ], ago);
  assert.equal(tile.count, "1 running");
  assert.deepEqual(tile.rows.map(r => [r.code, r.dot, r.note]), [["dev-server", "", "2 h"], ["logs", "off", "exited 1"]]);
  assert.equal(terminalTile([{ name: "x", state: "stopped", started_at: 1 }], ago).count, "1");
  assert.equal(terminalTile([], ago).count, "");
});

test("terminals start only where the project is checked out", () => {
  const where = [
    { kind: "here", id: "m1", name: "build-box", folder: "/srv/project" },
    { kind: "here", id: "m2", name: "mac-box", folder: "" },
    { kind: "other", id: "m3", name: "gpu-box", folder: "/srv/project" },
  ];
  assert.deepEqual(terminalPlaces(where), [{ id: "m1", name: "build-box", folder: "/srv/project" }]);
  assert.deepEqual(terminalPlaces(null), []);
});

test("output already on the screen is skipped, and a gap is noted", () => {
  // Fresh: everything from the start.
  assert.deepEqual(splice(0, 0, 10), { skip: 0, gap: false });
  // A reattach resent bytes 5..15 while the screen had 10.
  assert.deepEqual(splice(10, 5, 10), { skip: 5, gap: false });
  // All of it was there already.
  assert.deepEqual(splice(20, 5, 10), { skip: 10, gap: false });
  // The node no longer kept bytes 10..30.
  assert.deepEqual(splice(10, 30, 4), { skip: 0, gap: true });
});

test("output arrives as base64 bytes", () => {
  assert.deepEqual([...fromBase64("aGk=")], [104, 105]);
  assert.deepEqual([...fromBase64("G1szMm0=")], [27, 91, 51, 50, 109]);
  assert.equal(fromBase64("").length, 0);
});

test("reattaching waits longer each time, up to a cap", () => {
  assert.equal(retryDelay(1), 1000);
  assert.equal(retryDelay(2), 2000);
  assert.equal(retryDelay(3), 4000);
  assert.equal(retryDelay(10), 15000);
});

test("the footer says where typing goes", () => {
  assert.equal(linkLabel("attached", false, "build-box"), "Attached: what you type goes to build-box");
  assert.equal(linkLabel("attached", true, "build-box"), "Watching: typing is off");
  assert.equal(linkLabel("detached", false, "build-box"), "Detached: it keeps running");
});

test("every machine lists its terminals, and a removed one's go last", () => {
  const machines = [{ id: "m1", name: "build-box", online: true }, { id: "m2", name: "gpu-box", online: false }];
  const groups = byMachine(machines, [
    { id: "a", machine_id: "m1", machine_name: "build-box" },
    { id: "b", machine_id: "m9", machine_name: "old-box" },
    { id: "c", machine_id: "m1", machine_name: "build-box" },
  ]);
  assert.deepEqual(groups.map(m => [m.name, m.online, m.terms.map(t => t.id)]), [
    ["build-box", true, ["a", "c"]],
    ["gpu-box", false, []],
    ["old-box", false, ["b"]],
  ]);
  assert.equal(terminalsCount(groups), "3 on 2 machines");
  assert.equal(terminalsCount(byMachine(machines, [])), "None open");
});

test("a terminal says where it runs and how it stands", () => {
  assert.equal(terminalWhere({ project: "acme/widget", cwd: "/srv/project" }), "acme/widget · /srv/project");
  assert.equal(terminalWhere({ project: "", cwd: "~" }), "~");
  assert.equal(terminalNote({ state: "running", started_at: 1 }, ago), "2 h");
  assert.equal(terminalNote({ state: "exited", code: 0, started_at: 1 }, ago), "exited 0");
  assert.equal(terminalNote({ state: "exited", code: null, started_at: 1 }, ago), "ended");
});

test("a terminal that exited or is gone is done; an offline one is not", () => {
  assert.ok(endsForGood("exited"));
  assert.ok(endsForGood("stopped"));
  assert.ok(endsForGood("gone"));
  assert.ok(!endsForGood("offline"));
  assert.ok(!endsForGood("running"));
});
