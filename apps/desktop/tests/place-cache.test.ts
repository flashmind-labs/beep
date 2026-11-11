import { answers, calls } from "./bar-env.ts";
import test from "node:test";
import assert from "node:assert/strict";
import { onPlaces, placeOf } from "../src/agents/place.ts";

const wait = (ms: number) => new Promise<void>(resolve => setTimeout(resolve, ms));

test("600 session rows are asked for once each, in batches, not again on every redraw", async () => {
  const sessions = Array.from({ length: 600 }, (_, i) => ({ machine_id: "busy-box", agent: "claude", id: `session-${i}`, external: true }));
  const place = { folder: "/srv/project", main: "/srv/project", linked: false, branch: "main", origin: "" };
  const onBox = (args: unknown) => typeof args === "object" && args !== null && "machineId" in args && args.machineId === "busy-box";
  const asked = () => calls.filter(call => call.cmd === "agent_places" && onBox(call.args)).length;
  let draws = 0;
  // A view that draws every row again whenever places arrive, as the session lists do.
  onPlaces(() => { draws++; for (const s of sessions) placeOf(s); });

  try {
    answers.agent_places = { kind: "found", places: Array.from({ length: 50 }, () => place) };
    for (const s of sessions) placeOf(s);
    await wait(1500);
    const first = asked();
    // 600 sessions in batches of 50. A cache that emptied itself past 500
    // asked about 100 times in this second and a half, and never stopped.
    assert.equal(first, 12);
    await wait(1000);
    assert.equal(asked(), first, "no asks without a change");
    assert.ok(draws <= 12, `drew ${draws} times`);
  } finally {
    delete answers.agent_places;
  }
});
