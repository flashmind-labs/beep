import { answers, calls } from "./bar-env.ts";
import test from "node:test";
import assert from "node:assert/strict";
import { placeOf } from "../src/agents/place.ts";

const settled = () => new Promise<void>(resolve => setImmediate(resolve));

test("a visible pane backs off failed placement reads instead of retrying every redraw", async () => {
  const now = Date.now;
  let time = 100_000;
  Date.now = () => time;
  const session = { machine_id: "build-box", agent: "codex", id: "session" };
  const requests = () => calls.filter(call => call.cmd === "agent_place").length;

  try {
    answers.agent_place = Promise.reject("offline");
    placeOf(session, 5000);
    await settled();
    assert.equal(requests(), 1);
    time += 6000;
    placeOf(session, 5000);
    await settled();
    assert.equal(requests(), 1);
    time += 24_000;
    answers.agent_place = Promise.reject("offline");
    placeOf(session, 5000);
    await settled();
    assert.equal(requests(), 2);
    time += 30_000;
    placeOf(session, 5000);
    await settled();
    assert.equal(requests(), 2);
    time += 30_000;
    answers.agent_place = null;
    placeOf(session, 5000);
    await settled();
    assert.equal(requests(), 3);
  } finally {
    Date.now = now;
    delete answers.agent_place;
  }
});
