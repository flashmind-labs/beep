import test from "node:test";
import assert from "node:assert/strict";
import { joinLine, notJoinedSub, splitJoined } from "../src/machines/notjoined-state.ts";

test("a_machine_not_yet_joined_shows_its_join_line_and_still_works_through_the_server", () => {
  const machines = [
    { id: "a", name: "This Mac", joined: true },
    { id: "b", name: "build-box", joined: false },
    // A list from an app of before the field is all in the mesh.
    { id: "c", name: "gpu-box" },
  ];
  const { mesh, outside } = splitJoined(machines);
  assert.deepEqual(mesh.map(m => m.id), ["a", "c"]);
  assert.deepEqual(outside.map(m => m.id), ["b"]);
  assert.equal(notJoinedSub("Linux"), "Linux · still works through your account");
  assert.equal(notJoinedSub(""), "still works through your account");
  assert.equal(joinLine("beepjoin1example"), "beep-node mesh join beepjoin1example");
});
