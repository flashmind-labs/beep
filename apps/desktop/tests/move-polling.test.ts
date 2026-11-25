import { answers, calls } from "./steer-env.ts";
import assert from "node:assert/strict";
import test from "node:test";
import { moveStore, refreshMoves, stateOf, takeProgress, takePushedSessions } from "../src/agents/move-store.ts";
import type { MoveProgress, MoveRecord } from "../src/agents/move-state.ts";

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

test("progress comes from the moves push and is never asked for", async () => {
  const move = record("pushed");
  answers.session_moves = [move];
  await refreshMoves();
  assert.equal(stateOf("pushed"), null);

  takeProgress({ views: [{ command: "session_move_progress", args: { session: "pushed" }, value: progress("files") }] });
  assert.equal(stateOf("pushed")?.phase, "files");
  takeProgress({ views: [{ command: "session_move_progress", args: { session: "pushed" }, value: progress("resuming") }] });
  assert.equal(stateOf("pushed")?.phase, "resuming");
  assert.equal(checks(), 0);
});

test("progress pushed before its record waits for the record", () => {
  takeProgress({ views: [{ command: "session_move_progress", args: { session: "early" }, value: progress("files") }] });
  takePushedSessions([record("early")]);
  assert.equal(stateOf("early")?.phase, "files");

  takePushedSessions([]);
  assert.equal(stateOf("early"), null, "a move that ended takes its progress");
});

test("a pushed move list shows a move started elsewhere without a read", async () => {
  const reads = () => calls.filter(call => call.cmd === "session_moves").length;
  const move = record("chat");
  answers.session_move_progress = progress("files");

  takePushedSessions([move]);
  assert.equal(moveStore.getState().records.get("chat")?.to, "target");
  assert.equal(reads(), 0, "the push carries the moves, nothing is read");

  takePushedSessions([]);
  assert.equal(moveStore.getState().records.size, 0, "a move that ended leaves");
  assert.equal(reads(), 0);
});
