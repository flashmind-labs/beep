import test from "node:test";
import assert from "node:assert/strict";
import { type Note, type NoteKind, asks, badge, bellLabel, conflictProject, doneLabel, find, fromNote, markRead, noteAgent, opens, copies, copyLine, skillName, subLine, unreadCount, unreadIds, without } from "../src/stores/center/notices-state.ts";

const note = (id: string, kind: NoteKind, over: Partial<Note> = {}): Note => ({ id, kind, title: `Title ${id}`, text: "", html: "", source: "build-box", at: 1_760_000_000_000, read: false, done: false, target: null, allow_deny: true, ...over });
const items = (...ids: string[]) => ids.map(id => fromNote(note(id, "finished", { text: `Text ${id}` })));

test("the badge counts what is unread and clears when it is read", () => {
  const list = [fromNote(note("a", "finished")), fromNote(note("c", "finished", { read: true })), fromNote(note("b", "asks"))];
  assert.equal(unreadCount(list), 2);
  assert.equal(badge(list), "2");
  assert.equal(bellLabel(list), "Notifications, 2 unread");
  assert.deepEqual(unreadIds(list).sort(), ["a", "b"]);
  const read = markRead(list, ["a", "b"]);
  assert.equal(unreadCount(read), 0);
  assert.equal(badge(read), "");
  assert.equal(bellLabel(read), "Notifications");
  // The list it came from is not changed.
  assert.equal(unreadCount(list), 2);
});

test("the badge stops counting at 9", () => {
  const many = items(...Array.from({ length: 12 }, (_, i) => String(i)));
  assert.equal(badge(many), "9+");
});

test("dismissing leaves the others", () => {
  const list = items("a", "b", "c");
  assert.deepEqual(without(list, ["b"]).map(i => i.id), ["a", "c"]);
  assert.deepEqual(without(list, list.map(i => i.id)), []);
  assert.equal(without(list, ["gone"]).length, 3);
});

test("an opened row is found while the list has it", () => {
  const list = items("a", "b");
  assert.equal(find(list, "b")?.text, "Text b");
  assert.equal(find(without(list, ["b"]), "b"), null);
  assert.equal(find(list, null), null);
});

test("an ask has answers until it is answered, and what is over says so", () => {
  const open = fromNote(note("q", "asks"));
  assert.ok(asks(open));
  assert.equal(doneLabel(open), "");
  const answered = fromNote(note("q", "asks", { done: true }));
  assert.ok(!asks(answered));
  assert.equal(doneLabel(answered), "Answered");
  assert.equal(doneLabel(fromNote(note("c", "conflict", { done: true }))), "Settled");
  assert.equal(doneLabel(fromNote(note("m", "offline", { done: true }))), "Back online");
  assert.ok(!asks(fromNote(note("f", "finished"))));
  // A choice among answers only opens its chat.
  assert.ok(!asks(fromNote(note("c", "asks", { allow_deny: false }))));
});

test("a row opens what it is about", () => {
  const session = { agent: { machine_id: "m1", id: "s1" } } as unknown as Note["target"];
  assert.equal(opens(fromNote(note("f", "finished", { target: session }))), "session");
  assert.equal(opens(fromNote(note("v", "move_failed", { target: session }))), "session");
  // A sync conflict opens its project, named in the note's id.
  assert.equal(opens(fromNote(note("c", "conflict"))), "conflict");
  assert.equal(conflictProject(fromNote(note("conflict:acme/widget:build-box", "conflict"))), "acme/widget");
  assert.equal(conflictProject(fromNote(note("c", "conflict"))), null);
  assert.equal(opens(fromNote(note("m", "offline"))), "machines");
  // A temporary machine removed after it was gone a while.
  assert.equal(opens(fromNote(note("r", "removed"))), "machines");
  // A skill conflict opens in the pane.
  assert.equal(opens(fromNote(note("s", "skill_conflict"))), "skill");
  const run = { projects: { project: "acme/widget", run: { id: 7, host: "github.com" } } };
  assert.equal(opens(fromNote(note("r", "run", { target: run }))), "project");
});

test("a row says where it came from and when", () => {
  assert.equal(subLine(fromNote(note("a", "finished", { source: "build-box" })), "4 min ago"), "build-box · 4 min ago");
});

test("a coding agent's row shows its agent and only when", () => {
  const agent = { machine_id: "m1", machine_name: "build-box", id: "s1", agent: "claude", cwd: "/srv/widget", status: "idle", external: false };
  const done = fromNote(note("a", "finished", { target: { agent } }));
  assert.equal(noteAgent(done), "claude");
  assert.equal(subLine(done, "4 min ago"), "4 min ago");
  assert.equal(noteAgent(fromNote(note("b", "offline"))), null);
});

test("a thread's row names its chat", () => {
  const agent = { machine_id: "m1", machine_name: "build-box", id: "t1", agent: "claude", cwd: "/srv/chat/pricing", status: "asking", external: false };
  assert.equal(subLine(fromNote(note("a", "asks", { target: { agent } })), "4 min ago", "Plan the launch"), "Plan the launch · 4 min ago");
});

test("a skill conflict lists its copies and which are on this computer", () => {
  const i = fromNote(note("skill:notes", "skill_conflict", { title: "Skill notes differs on box-a and box-b", text: "box-a ~/.claude/skills\nbox-b ~/.agents/skills", here: ["box-a ~/.claude/skills"] }));
  assert.equal(skillName(i), "notes");
  assert.deepEqual(copies(i), [
    { machine: "box-a", folder: "~/.claude/skills", here: true },
    { machine: "box-b", folder: "~/.agents/skills", here: false },
  ]);
  // A note saved before `here` existed has no copy on this computer.
  assert.ok(copies(fromNote(note("o", "skill_conflict", { text: "box-a ~/x" }))).every(c => !c.here));
});

test("a skill conflict's copies say when they changed, their size and the newest", () => {
  const now = 10 * 3600_000;
  const held = (label: string, changed: number) => ({ machine: `id-${label}`, label, folder: "~/.claude/skills", hash: label, changed, bytes: 2100 });
  const i = fromNote(note("skill:notes", "skill_conflict", { here: ["box-a ~/.claude/skills"], held: [held("box-a", now - 3 * 3600_000), held("box-b", now - 120_000)] }));
  const [a, b] = copies(i);
  assert.ok(a.here && !a.newest && !b.here && b.newest);
  assert.equal(copyLine(a, now), "Changed 3 h ago · 2.1 KB");
  assert.equal(copyLine(b, now), "Changed 2 min ago · 2.1 KB · newest");
});
