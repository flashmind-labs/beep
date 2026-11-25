import test from "node:test";
import assert from "node:assert/strict";
import type { AgentSession } from "../src/agents/agents-state.ts";
import { type MoveProgress, type MoveRecord, type MoveTarget, bySession, canMove, chatSessions, movePct, dedupe, effective, fmtBytes, going, listLabel, moveRows, movedTo, targetNote } from "../src/agents/move-state.ts";

const progress = (over: Partial<MoveProgress> = {}): MoveProgress => ({ answer: "known", phase: "waiting", copied: 0, total: 0, source_away: false, reason: null, worktree: null, worktree_at: 0, worktrees: 0, edits_left: false, files_copied: 0, files_total: 0, ...over });
const record: MoveRecord = { session: "s1", agent: "codex", from: "laptop-id", to: "studio-id", project: "acme/app", at: 10 };
const names = (id: string) => id.replace("-id", "");
const target = (over: Partial<MoveTarget> = {}): MoveTarget => ({ id: "studio-id", name: "studio", online: true, harnesses: ["codex", "claude"], checkout: true, ...over });

test("only Claude Code, Codex, pi and flsh sessions move", () => {
  assert.ok(canMove({ agent: "pi" }));
  assert.ok(canMove({ agent: "flsh" }));
  assert.ok(!canMove({ agent: "opencode" }));
});

test("the menu says why a machine cannot take the session", () => {
  assert.deepEqual(targetNote(target(), ["codex"]), { note: "folder ready", enabled: true, offline: false });
  assert.deepEqual(targetNote(target({ checkout: false }), ["codex"]), { note: "no checkout: import first", enabled: false, offline: false });
  assert.deepEqual(targetNote(target({ harnesses: ["claude"] }), ["codex"]), { note: "no Codex here", enabled: false, offline: false });
  assert.deepEqual(targetNote(target({ online: false }), ["codex"]), { note: "offline: moves when it returns", enabled: true, offline: true });
  assert.equal(targetNote(target({ old: true }), ["codex"]).enabled, false);
});

test("a machine with no checkout is named before one that is offline", () => {
  assert.equal(targetNote(target({ checkout: false, online: false }), ["codex"]).note, "no checkout: import first");
});

test("the steps follow the phases", () => {
  assert.deepEqual(moveRows(progress({ phase: "conversation", copied: 4_200_000, total: 6_100_000 }), "laptop", "studio"), [
    { state: "run", text: "Conversation 4.2 MB of 6.1 MB" },
    { state: "todo", text: "Checkouts and worktrees" },
    { state: "todo", text: "Resuming on studio" },
  ]);
  const files = moveRows(progress({ phase: "files", total: 6_100_000 }), "laptop", "studio");
  assert.deepEqual(files.map(r => r.state), ["done", "run", "todo"]);
  assert.equal(files[1].text, "Checkouts");
  const tree = moveRows(progress({ phase: "files", worktree: "beep-tools", worktree_at: 1, worktrees: 2 }), "laptop", "studio");
  assert.equal(tree[1].text, "Worktree beep-tools 1 of 2");
  const resuming = moveRows(progress({ phase: "resuming", total: 6_100_000 }), "laptop", "studio");
  assert.deepEqual(resuming.map(r => r.state), ["done", "done", "run"]);
  const left = moveRows(progress({ phase: "resuming", total: 6_100_000, edits_left: true }), "laptop", "studio");
  assert.deepEqual(left.at(-1), { state: "note", text: "Uncommitted edits in the project folder stay on laptop" });
});

test("a chat's move needs every harness its threads use", () => {
  assert.deepEqual(targetNote(target({ harnesses: ["flsh"] }), ["flsh", "claude"]), { note: "no Claude Code here", enabled: false, offline: false });
  assert.equal(targetNote(target({ harnesses: ["flsh", "claude"] }), ["flsh", "claude"]).enabled, true);
});

test("a chat's steps bring its files, not checkouts", () => {
  assert.equal(moveRows(progress(), "laptop", "studio", true, "chat")[2].text, "Files");
  assert.equal(moveRows(progress({ phase: "files", total: 1_000_000 }), "laptop", "studio", true, "chat")[1].text, "Files");
});

test("a chat's files count up, with their speed once they take over five seconds", () => {
  const copying = progress({ phase: "files", total: 1_000_000, files_copied: 120_000_000, files_total: 551_000_000 });
  assert.equal(moveRows(copying, "laptop", "studio", true, "chat")[1].text, "Files 120.0 MB of 551.0 MB · 22%");
  const slow = moveRows(copying, "laptop", "studio", true, "chat", { since: 0, now: 10_000 })[1].text;
  assert.match(slow, /^Files 120\.0 MB of 551\.0 MB · 22% · [\d.]+ MB\/s$/);
  assert.equal(moveRows(progress({ phase: "files", files_total: 0 }), "laptop", "studio", true, "chat")[1].text, "Files");
});

test("the bar over the input fills with the step that copies", () => {
  assert.equal(movePct(progress({ phase: "conversation", copied: 3, total: 4 })), 75);
  assert.equal(movePct(progress({ phase: "files", files_copied: 1, files_total: 4 })), 25);
  assert.equal(movePct(progress({ phase: "files" })), null, "a project's checkouts have no size");
  assert.equal(movePct(progress({ phase: "waiting" })), null);
  assert.equal(movePct(progress({ phase: "conversation", copied: 9, total: 4 })), 99, "full only once it is done");
});

const CHAT = "/home/u/.beep/assistant/c1";
const row = (over: Partial<AgentSession>): AgentSession => ({
  machine_id: "laptop-id", machine_name: "laptop", id: "x", agent: "flsh", title: null, cwd: CHAT, status: "idle", doing: null,
  started_at: 0, last_activity: 0, external: false, question: null, assistant: false, persona: null, ...over,
});
const under = (id: string) => ({ machine_id: "laptop-id", id, key: "" });

test("a chat moves with its threads and theirs, the chat first, but not with a reviewer", () => {
  const chat = row({ id: "c1", assistant: true });
  const deep = row({ id: "t2", agent: "codex", cwd: `${CHAT}/research/deep`, parent: under("t1"), started_at: 3, status: "ended" });
  const thread = row({ id: "t1", agent: "claude", cwd: `${CHAT}/research`, parent: under("c1"), started_at: 2 });
  const reviewer = row({ id: "r1", agent: "claude", cwd: "/srv/project", parent: under("c1"), reviewer: true });
  const other = row({ id: "o1", assistant: true, cwd: "/home/u/.beep/assistant/o1" });

  assert.deepEqual(chatSessions([deep, other, reviewer, thread, chat], chat), { sessions: [chat, thread, deep], agents: ["flsh", "claude", "codex"] });
});

test("a chat with a thread on a third machine does not move", () => {
  const chat = row({ id: "c1", assistant: true });
  const away = row({ id: "t1", machine_id: "studio-id", machine_name: "studio", cwd: `${CHAT}/research`, parent: under("c1") });

  assert.deepEqual(chatSessions([chat, away], chat), { refused: "a thread of this chat is on studio" });
});

test("a thread in a project folder blocks the move while it runs, and stays behind once it ended", () => {
  const chat = row({ id: "c1", assistant: true });
  const coding = row({ id: "t1", agent: "codex", cwd: "/srv/project", parent: under("c1"), status: "working" });

  assert.deepEqual(chatSessions([chat, coding], chat), { refused: "a thread works outside the chat's folder until it ends" });
  assert.deepEqual(chatSessions([chat, { ...coding, status: "ended" }], chat), { sessions: [chat], agents: ["flsh"] });
});

test("a move waits for the source's turn, or for the source itself", () => {
  assert.equal(moveRows(progress(), "laptop", "studio")[0].text, "Waiting for studio");
  assert.equal(moveRows(progress(), "laptop", "studio", false)[0].text, "studio is offline; the move goes on when it is back");
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

test("the checkouts step says what it waits for", () => {
  const reason = "Waiting for checkouts to match: laptop at abc1234, here def5678";
  const rows = moveRows(progress({ phase: "files", total: 1_000_000, reason }), "laptop", "studio");
  assert.equal(rows[1].text, reason);
});

test("an open session follows its move once the new machine lists it", () => {
  const r: MoveRecord = { session: "s1", agent: "codex", from: "box", to: "air", project: "acme/widget", at: 1 };
  const records = bySession([r]);
  const open = { machine_id: "box", session: "s1", agent: "codex" };
  const there = { machine_id: "air", id: "s1", agent: "codex" };
  assert.equal(movedTo(open, records, [{ machine_id: "box", id: "s1", agent: "codex" }]), null);
  assert.equal(movedTo(open, records, [there]), there);
  assert.equal(movedTo({ ...open, machine_id: "air" }, records, [there]), null);
});
