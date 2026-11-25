import { answers, calls } from "./bar-env.ts";
import test from "node:test";
import assert from "node:assert/strict";
import { onPlaces, placeOf, takePlaces, takeSessionPlaces } from "../src/agents/place.ts";

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

test("a place is asked once, kept however old, and replaced by a push", async () => {
  const now = Date.now;
  const session = { machine_id: "pushed-box", agent: "codex", id: "one" };
  const place = (folder: string) => ({ folder, main: "/srv/project", linked: folder !== "/srv/project", branch: "main", origin: "" });
  const asks = () => calls.filter(call => call.cmd === "agent_places" && JSON.stringify(call.args).includes("pushed-box")).length;
  let drawn = 0;
  onPlaces(() => drawn++);

  try {
    answers.agent_places = { kind: "found", places: [place("/srv/project")] };
    placeOf(session);
    await wait(200);
    assert.equal(asks(), 1);

    Date.now = () => now() + 3_600_000;
    assert.equal(placeOf(session)?.folder, "/srv/project");
    await wait(10);
    assert.equal(asks(), 1, "an old answer is not asked again");

    takePlaces({ views: [{ command: "agent_places", args: { machineId: "pushed-box", asks: [{ agent: "codex", session: "one", external: false }] }, value: { kind: "found", places: [place("/srv/project/.worktrees/fix")] } }] });
    assert.equal(placeOf(session)?.folder, "/srv/project/.worktrees/fix");
    await wait(200);
    assert.equal(drawn, 2);
    assert.equal(asks(), 1);
  } finally {
    Date.now = now;
    delete answers.agent_places;
  }
});

test("a Beep session's place comes with its row and a moved one replaces it, with no ask", () => {
  const row = (folder: string, external = false) => ({ machine_id: "row-box", agent: "claude", id: "beep-1", external, place: { folder, main: "/srv/project", linked: folder !== "/srv/project", branch: "main", origin: "" } });
  const asks = () => calls.filter(call => call.cmd === "agent_places" && JSON.stringify(call.args).includes("row-box")).length;

  takeSessionPlaces([row("/srv/project")]);
  assert.equal(placeOf(row(""))?.folder, "/srv/project");

  takeSessionPlaces([row("/srv/project/.worktrees/fix")]);
  assert.equal(placeOf(row(""))?.folder, "/srv/project/.worktrees/fix");
  assert.equal(asks(), 0);

  // A session started outside Beep is read from its machine, not from a row.
  takeSessionPlaces([{ ...row("/elsewhere", true), id: "outside-1" }]);
  assert.equal(placeOf({ machine_id: "row-box", agent: "claude", id: "outside-1", external: true }), null);
});
