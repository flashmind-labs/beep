import test from "node:test";
import assert from "node:assert/strict";
import { OLDER_MAX, answersBefore, joinRuns, lacksPrompt, olderBefore, olderNote, takeOlder } from "../src/agents/older-state.ts";
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
  const total = 530;
  let held = run(total - 60, total);
  while (held.earlier > 0) {
    const before = held.earlier, from = Math.max(0, before - 200);
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

test("the log holds at most its bound and stops asking for older entries there", () => {
  assert.equal(olderBefore(run(100, OLDER_MAX + 100), ""), null);
  const joined = joinRuns(run(100, OLDER_MAX + 90), run(OLDER_MAX + 80, OLDER_MAX + 130));
  assert.equal(joined.entries.length, OLDER_MAX);
  assert.equal(joined.earlier, 130);
  assert.equal(joined.entries[0].text, "e130");
  assert.equal(takeOlder(run(100, OLDER_MAX), run(0, 100)).stop, "full");
});

test("the oldest turn lacks its prompt until the run starts with one or reaches the start", () => {
  assert.equal(lacksPrompt(run(40, 100)), true);
  assert.equal(lacksPrompt({ earlier: 40, entries: [{ kind: "user", text: "go" }, ...at(41, 100)] }), false);
  assert.equal(lacksPrompt(run(0, 100)), false);
  assert.equal(lacksPrompt({ earlier: 5, entries: [] }), false);
});

test("a prompt search that reads pages until a user entry comes first ends within its bound", () => {
  // Three turns of 120 entries, the newest 60 held: the first read still starts mid-turn.
  const log = [0, 1, 2].flatMap(n => [{ kind: "user", text: `p${n}` }, ...at(0, 119)]);
  const total = log.length;
  let held = { earlier: total - 60, entries: log.slice(total - 60) };
  let reads = 0;
  while (lacksPrompt(held) && olderBefore(held, "") != null && reads < 5) {
    const before = held.earlier, from = Math.max(0, before - 200);
    held = takeOlder(held, { earlier: from, entries: log.slice(from, before) }).run;
    reads++;
  }
  assert.equal(held.entries[0].kind, "user");
  assert.equal(reads, 2);
  assert.equal(splitTurns(held.entries, held.earlier)[0].user, held.earlier);
});

test("a prompt search gives up at its bound on a turn with no prompt in reach", () => {
  const log = at(0, 4000);
  let held = { earlier: 3940, entries: log.slice(3940) };
  let reads = 0;
  while (lacksPrompt(held) && olderBefore(held, "") != null && reads < 5) {
    const before = held.earlier, from = Math.max(0, before - 200);
    held = takeOlder(held, { earlier: from, entries: log.slice(from, before) }).run;
    reads++;
  }
  assert.equal(reads, 5);
  assert.equal(held.earlier, 3940 - 1000);
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
