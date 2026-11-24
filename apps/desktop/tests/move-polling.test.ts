import { answers, calls } from "./steer-env.ts";
import assert from "node:assert/strict";
import test from "node:test";
import { moveStore, refreshMoves, stateOf, takePushedSessions } from "../src/agents/move-store.ts";
import type { MoveProgress, MoveRecord } from "../src/agents/move-state.ts";
import { SLOW_MS } from "../src/ui/active.ts";

const record = (session: string): MoveRecord => ({
  session, agent: "codex", from: "source", to: "target", project: "example", at: Date.now() - 600_000,
});
const progress = (phase: MoveProgress["phase"]): MoveProgress => ({
  answer: "known", phase, copied: 0, total: 0, source_away: false, reason: null, worktree: null,
  worktree_at: 0, worktrees: 0, edits_left: false, files_copied: 0, files_total: 0,
});
const checks = () => calls.filter(call => call.cmd === "session_move_progress").length;

test.beforeEach(() => {
  calls.length = 0;
  answers.agent_hosts = [];
  moveStore.setState({ records: new Map(), progress: new Map() });
});

test("list refreshes share checks and reach moves beyond the first batch", async t => {
  t.mock.timers.enable({ apis: ["Date"], now: 1_000_000 });
  const records = Array.from({ length: 9 }, (_, index) => record(String(index)));
  answers.session_moves = records;
  answers.session_move_progress = progress("done");

  await Promise.all([refreshMoves(), refreshMoves()]);
  assert.equal(checks(), 9);
  for (let i = 0; i < 275; i++) {
    t.mock.timers.tick(1_740);
    answers.session_moves = records.map(r => ({ ...r }));
    await refreshMoves();
  }
  assert.equal(checks(), 18);

  answers.session_moves = [{ ...records[0], at: Date.now() }];
  answers.session_move_progress = progress("files");
  await refreshMoves();
  assert.equal(checks(), 19);
  assert.equal(stateOf("0")?.phase, "files");
  assert.equal(stateOf("1"), null);

  t.mock.timers.tick(SLOW_MS);
  answers.session_move_progress = progress("resuming");
  await refreshMoves();
  assert.equal(stateOf("0")?.phase, "resuming");
});

test("a cancelled move stays gone when its progress request returns", async () => {
  const move = record("cancelled");
  answers.session_moves = [move];
  let finish: (answer: MoveProgress) => void = () => assert.fail("progress request is missing");
  answers.session_move_progress = new Promise<MoveProgress>(resolve => { finish = resolve; });
  const read = refreshMoves();
  await new Promise<void>(resolve => setImmediate(resolve));
  assert.equal(checks(), 1);

  moveStore.setState({ records: new Map(), progress: new Map() });
  finish(progress("files"));
  await read;
  assert.equal(stateOf(move.session), null);
});

test("a pushed list reads the moves when it shows a move change, not on every push", async () => {
  const row = (machine_id: string) => ({ machine_id, id: "chat", agent: "codex" });
  const reads = () => calls.filter(call => call.cmd === "session_moves").length;
  const settle = () => new Promise<void>(resolve => setImmediate(resolve));
  answers.session_moves = [];

  takePushedSessions([row("source")]);
  await settle();
  assert.equal(reads(), 1, "the first push reads once");

  takePushedSessions([row("source")]);
  takePushedSessions([row("source"), { ...row("source"), id: "other" }]);
  await settle();
  assert.equal(reads(), 1, "the same placement reads nothing");

  takePushedSessions([row("source"), row("target")]);
  await settle();
  assert.equal(reads(), 2, "a session on two machines is a move starting");

  takePushedSessions([row("target")]);
  await settle();
  assert.equal(reads(), 3, "the source row going is the move ending");
});
