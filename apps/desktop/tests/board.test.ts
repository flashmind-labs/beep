import test from "node:test";
import assert from "node:assert/strict";
import { taskTile, askingSessions, sessionKey } from "../src/project/board.ts";

test("a task whose worker asks gets the asks chip, before review or blocked", () => {
  const asking = askingSessions(
    [{ machine_id: "m1", id: "s1", status: "asking" }, { machine_id: "m1", id: "s2", status: "working" }],
    [{ kind: "agent", machine_id: "m2", session: "s3" }, { kind: "chat" }],
  );
  assert.deepEqual([...asking].sort(), [sessionKey("m1", "s1"), sessionKey("m2", "s3")]);
  const workers = [
    { id: "w1", machine_id: "m1", session: "s1" },
    { id: "w2", machine_id: "m1", session: "s2" },
    { id: "w3", machine_id: "m2", session: "s3" },
    { id: "w4", machine_id: "m1", session: null },
  ];
  const tile = taskTile([
    { id: "a", title: "asks", state: "running", worker: "w1" },
    { id: "b", title: "works", state: "running", worker: "w2" },
    { id: "c", title: "asks in review", state: "review", worker: "w3" },
    { id: "d", title: "not started", state: "running", worker: "w4" },
  ], workers, asking);
  assert.deepEqual(tile.rows.map(r => r.id), ["a", "b", "d", "c"]);
  assert.deepEqual(tile.rows.map(r => r.chip?.text ?? null), ["asks", null, null, "asks"]);
  assert.equal(taskTile([{ id: "c", title: "x", state: "review", worker: "w3" }], workers, new Set()).rows[0].chip!.text, "review");
});
