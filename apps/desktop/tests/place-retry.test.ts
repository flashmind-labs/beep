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
  const requests = () => calls.filter(call => call.cmd === "agent_places").length;

  try {
    answers.agent_places = Promise.reject("offline");
    placeOf(session);
    await settled();
    assert.equal(requests(), 1);
    time += 6000;
    placeOf(session);
    await settled();
    assert.equal(requests(), 1);
    time += 24_000;
    answers.agent_places = Promise.reject("offline");
    placeOf(session);
    await settled();
    assert.equal(requests(), 2);
    time += 30_000;
    placeOf(session);
    await settled();
    assert.equal(requests(), 2);
    time += 30_000;
    answers.agent_places = { kind: "found", places: [null] };
    placeOf(session);
    await settled();
    assert.equal(requests(), 3);
  } finally {
    Date.now = now;
    delete answers.agent_places;
  }
});

test("a machine that does not answer yet is asked again soon, all its sessions in one call", async () => {
  const now = Date.now;
  let time = 1_000_000;
  Date.now = () => time;
  const sessions = ["one", "two", "three"].map(id => ({ machine_id: "starting-box", agent: "codex", id }));
  const onBox = (args: unknown) => typeof args === "object" && args !== null && "machineId" in args && args.machineId === "starting-box";
  const asked = () => calls.filter(call => call.cmd === "agent_places" && onBox(call.args));
  const sessionsAsked = (args: unknown) => typeof args === "object" && args !== null && "asks" in args && Array.isArray(args.asks) ? args.asks.length : 0;

  try {
    answers.agent_places = { kind: "unreachable" };
    for (const s of sessions) placeOf(s);
    await settled();
    assert.equal(asked().length, 1);
    time += 6000;
    answers.agent_places = { kind: "found", places: [null, null, null] };
    for (const s of sessions) placeOf(s);
    await settled();
    assert.equal(asked().length, 2);
    assert.equal(sessionsAsked(asked()[1].args), 3);
  } finally {
    Date.now = now;
    delete answers.agent_places;
  }
});
