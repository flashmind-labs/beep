import test from "node:test";
import assert from "node:assert/strict";
import { stateLabel, terminalTile, terminalPlaces } from "../src/terminals.ts";

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
