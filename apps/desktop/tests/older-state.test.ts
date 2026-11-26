import test from "node:test";
import assert from "node:assert/strict";
import { OLDER_PAGE, answersBefore, joinRuns, newestRead, olderBefore, readsOlder, viewRead, olderNote, takeOlder } from "../src/agents/older-state.ts";
import { onlyNotes, splitTurns } from "../src/agents/agentturns.ts";

const at = (from: number, to: number, kind = "agent") => Array.from({ length: to - from }, (_, i) => ({ kind, text: `e${from + i}` }));
const run = (earlier: number, to: number, kind = "agent") => ({ earlier, entries: at(earlier, to, kind) });

test("the newest read replaces the held entries from its place on and keeps what is older", () => {
  const held = run(100, 300);
  const fresh = { ...run(280, 330), session: "s" };
  const joined = joinRuns(held, fresh);
  assert.equal(joined.earlier, 100);
  assert.equal(joined.entries.length, 230);
  assert.deepEqual(joined.entries.map(e => e.text), at(100, 330).map(e => e.text));
  assert.equal(joined.session, "s");
});

test("a read that leaves a gap or starts earlier stands alone", () => {
  assert.equal(joinRuns(run(100, 160), run(400, 460)).earlier, 400);
  assert.equal(joinRuns(run(100, 160), run(40, 100)).earlier, 40);
  assert.equal(joinRuns(null, run(7, 9)).earlier, 7);
});

test("pages read before the first entry join to the whole log without a gap or a repeat", () => {
  const total = 5530;
  let held = run(total - 60, total);

  for (let before = olderBefore(held, ""); before != null; before = olderBefore(held, "")) {
    const from = Math.max(0, before - OLDER_PAGE);
    const got = run(from, before);
    assert.equal(answersBefore(got, before), true);
    held = takeOlder(held, got).run;
  }

  assert.deepEqual(held.entries.map(e => e.text), at(0, total).map(e => e.text));
  // A poll later keeps them.
  const polled = joinRuns(held, run(total - 55, total + 4));
  assert.equal(polled.entries.length, total + 4);
  assert.equal(polled.earlier, 0);
});

test("a node that does not read older entries answers the newest, which stops the loading", () => {
  const held = run(470, 530);
  const out = takeOlder(held, run(470, 530));
  assert.equal(out.stop, "old");
  assert.equal(out.run, held);
  assert.equal(olderBefore(out.run, out.stop), null);
  assert.match(olderNote(out.run, out.stop, false), /cannot be read from this machine/);
});

test("an empty answer says the agent no longer keeps them", () => {
  const held = run(10, 60);
  const out = takeOlder(held, { earlier: 10, entries: [] });
  assert.equal(out.stop, "gone");
  assert.equal(olderBefore(out.run, out.stop), null);
  assert.match(olderNote(out.run, out.stop, false), /no longer keeps/);
});

test("a user's prompt starts a turn and shows whatever the turn holds", () => {
  const log = [{ kind: "user", text: "go" }, { kind: "other", text: "[compacted]" }, { kind: "user", text: "again" }];

  for (const t of splitTurns(log)) {
    assert.notEqual(t.user, null);
    // Notes alone are shown bare only when no prompt starts the turn.
    assert.equal(onlyNotes(log, t), false);
  }
});

test("a run that starts mid-turn has no prompt for its first turn", () => {
  const turns = splitTurns(at(40, 50), 40);
  assert.equal(turns[0].user, null);
  assert.equal(turns[0].key, 40);
});

test("a session opens with a view's worth of its newest entries, then polls ask for fewer and keep it", () => {
  assert.equal(viewRead(1000), 70);
  assert.equal(viewRead(0), 20);
  assert.equal(viewRead(10_000), OLDER_PAGE);
  assert.equal(newestRead(null, 60, 1000), 70);
  assert.equal(newestRead({ earlier: 0, entries: [] }, 60, 1000), 70);
  const total = 900;
  const opened = run(total - newestRead(null, 60, 1000), total);
  assert.equal(olderBefore(opened, ""), total - 70);
  assert.equal(newestRead(opened, 60, 1000), 60);
  const polled = joinRuns(opened, run(total - 58, total + 2));
  assert.equal(polled.earlier, total - 70);
  assert.equal(polled.entries.length, 72);
});

test("earlier entries are read near the top, or all at once, and never while hidden", () => {
  assert.equal(readsOlder(false, true, 0), true);
  assert.equal(readsOlder(false, true, 899), true);
  assert.equal(readsOlder(false, true, 900), false);
  assert.equal(readsOlder(false, true, null), false);
  assert.equal(readsOlder(true, true, 5000), true);
  assert.equal(readsOlder(true, false, 0), false);
});
