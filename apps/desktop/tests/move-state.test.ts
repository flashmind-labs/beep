import test from "node:test";
import assert from "node:assert/strict";
import { type MoveProgress, type MoveRecord, type MoveTarget, bySession, canMove, dedupe, effective, fmtBytes, going, listLabel, moveRows, targetNote } from "../src/agents/move-state.ts";

const progress = (over: Partial<MoveProgress> = {}): MoveProgress => ({ answer: "known", phase: "waiting", copied: 0, total: 0, files_left: 0, source_away: false, reason: null, ...over });
const record: MoveRecord = { session: "s1", agent: "codex", from: "laptop-id", to: "studio-id", project: "acme/app", at: 10 };
const names = (id: string) => id.replace("-id", "");
const target = (over: Partial<MoveTarget> = {}): MoveTarget => ({ id: "studio-id", name: "studio", online: true, harnesses: ["codex", "claude"], checkout: true, ...over });

test("only Claude Code, Codex and pi sessions move", () => {
  assert.ok(canMove({ agent: "pi" }));
  assert.ok(!canMove({ agent: "opencode" }));
  assert.ok(!canMove({ agent: "flsh" }));
});

test("the menu says why a machine cannot take the session", () => {
  assert.deepEqual(targetNote(target(), "codex"), { note: "folder ready", enabled: true, offline: false });
  assert.deepEqual(targetNote(target({ checkout: false }), "codex"), { note: "no checkout: import first", enabled: false, offline: false });
  assert.deepEqual(targetNote(target({ harnesses: ["claude"] }), "codex"), { note: "no Codex here", enabled: false, offline: false });
  assert.deepEqual(targetNote(target({ online: false }), "codex"), { note: "offline: moves when it returns", enabled: true, offline: true });
  assert.equal(targetNote(target({ old: true }), "codex").enabled, false);
});

test("a machine with no checkout is named before one that is offline", () => {
  assert.equal(targetNote(target({ checkout: false, online: false }), "codex").note, "no checkout: import first");
});

test("the steps follow the phases", () => {
  assert.deepEqual(moveRows(progress({ phase: "conversation", copied: 4_200_000, total: 6_100_000 }), "laptop", "studio"), [
    { state: "run", text: "Conversation 4.2 MB of 6.1 MB" },
    { state: "todo", text: "Project files" },
    { state: "todo", text: "Resuming on studio" },
  ]);
  const files = moveRows(progress({ phase: "files", total: 6_100_000, files_left: 12 }), "laptop", "studio");
  assert.deepEqual(files.map(r => r.state), ["done", "run", "todo"]);
  assert.equal(files[1].text, "Project files 12 left");
  const resuming = moveRows(progress({ phase: "resuming", total: 6_100_000 }), "laptop", "studio");
  assert.deepEqual(resuming.map(r => r.state), ["done", "done", "run"]);
});

test("a move waits for the source's turn, or for the source itself", () => {
  assert.equal(moveRows(progress(), "laptop", "studio")[0].text, "Waiting for the turn to end");
  assert.equal(moveRows(progress({ source_away: true }), "laptop", "studio")[0].text, "Waiting for laptop");
});

test("sizes read as megabytes or kilobytes", () => {
  assert.equal(fmtBytes(6_100_000), "6.1 MB");
  assert.equal(fmtBytes(40_000), "40 KB");
  assert.equal(fmtBytes(0), "1 KB");
});

test("a move under way shows the machines and a chip; a failed one stays on its source with the reason", () => {
  assert.deepEqual(listLabel(record, progress({ phase: "files" }), names), { sub: "laptop to studio", chip: "Moving", why: null });
  const failed = listLabel(record, progress({ phase: "failed", reason: "studio went offline during the file copy" }), names);
  assert.equal(failed?.sub, "laptop");
  assert.equal(failed?.chip, "Move failed");
  assert.equal(failed?.why, "Move failed: studio went offline during the file copy. Still on laptop.");
});

test("a move that finished or was cancelled, or one nobody answered for, says nothing", () => {
  assert.equal(listLabel(record, progress({ phase: "done" }), names), null);
  assert.equal(listLabel(record, progress({ phase: "cancelled" }), names), null);
  assert.equal(listLabel(record, progress({ answer: "unknown" }), names), null);
  assert.equal(listLabel(undefined, progress(), names), null);
  assert.ok(going(progress({ phase: "resuming" })));
  assert.ok(!going(progress({ phase: "done" })));
  assert.ok(!going(progress({ answer: "update" })));
});

test("one id on two machines shows once, on the machine the record names", () => {
  const rows = [
    { machine_id: "laptop-id", id: "s1", agent: "codex" },
    { machine_id: "studio-id", id: "s1", agent: "codex" },
    { machine_id: "laptop-id", id: "s2", agent: "codex" },
  ];
  assert.deepEqual(dedupe(rows, [record]).map(r => `${r.machine_id}:${r.id}`), ["studio-id:s1", "laptop-id:s2"]);
});

test("a session the target does not list yet stays on its source", () => {
  const rows = [{ machine_id: "laptop-id", id: "s1", agent: "codex" }];
  assert.equal(dedupe(rows, [record]).length, 1);
});

test("the latest record of a session wins", () => {
  const later = { ...record, from: "studio-id", to: "laptop-id", at: 20 };
  assert.equal(bySession([later, record]).get("s1"), later);
});

test("a target that has not heard of a recent move yet is waited for, and an old silence says nothing", () => {
  const unknown = progress({ answer: "unknown" });
  assert.equal(effective(unknown, record, record.at + 30_000)?.phase, "waiting");
  assert.ok(going(effective(unknown, record, record.at + 30_000)));
  assert.equal(effective(unknown, record, record.at + 10 * 60_000), null);
  assert.equal(effective(undefined, record, record.at), null);
  assert.equal(effective(progress({ phase: "files" }), record, record.at + 10 * 60_000)?.phase, "files");
});

test("a node from before moves shows the move as failed, with the session still on its source", () => {
  const old = effective(progress({ answer: "update" }), record, record.at + 1000);
  assert.equal(old?.phase, "failed");
  const label = listLabel(record, old, names);
  assert.equal(label?.chip, "Move failed");
  assert.match(label?.why ?? "", /too old.*Still on laptop/);
  assert.ok(!going(old));
});

test("the files step says why they do not come", () => {
  const rows = moveRows(progress({ phase: "files", total: 1_000_000, files_left: 3, reason: "this checkout is at abc and another machine's at def" }), "laptop", "studio");
  assert.equal(rows[1].text, "Project files: this checkout is at abc and another machine's at def");
});

test("a machine whose Syncthing leaves the worktree out cannot take it", () => {
  assert.deepEqual(targetNote(target({ ignored: true }), "codex"), { note: "Syncthing is set not to sync this worktree", enabled: false, offline: false });
  assert.equal(targetNote(target({ ignored: true, old: true }), "codex").note, "update this machine first");
});
