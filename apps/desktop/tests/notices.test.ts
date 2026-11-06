import test from "node:test";
import assert from "node:assert/strict";
import { type Alert, type Note, type NoteKind, asks, badge, bellLabel, bySide, doneLabel, find, fromAlert, fromNote, markRead, merge, opens, copies, skillName, subLine, unreadCount, unreadIds, without } from "../src/center/notices-state.ts";

const alert = (id: string, over: Partial<Alert> = {}): Alert => ({ session_id: id, title: `Title ${id}`, source: "Apps", text: `Text ${id}`, read: false, at: 1_760_000_000_000, ...over });
const note = (id: string, kind: NoteKind, over: Partial<Note> = {}): Note => ({ id, kind, title: `Title ${id}`, text: "", source: "build-box", at: 1_760_000_000_000, read: false, done: false, target: null, ...over });
const items = (...list: Alert[]) => list.map(fromAlert);

test("alerts and the mesh's notes are one list, newest first", () => {
  const list = merge(items(alert("a", { at: 3 }), alert("b", { at: 1 })), [note("n", "finished", { at: 2 })].map(fromNote));
  assert.deepEqual(list.map(i => i.id), ["a", "n", "b"]);
  assert.deepEqual(list.map(i => i.server), [true, false, true]);
  // With no server the notes alone fill the pane.
  assert.deepEqual(merge(null, [note("n", "offline")].map(fromNote)).map(i => i.id), ["n"]);
  assert.deepEqual(merge(null, []), []);
});

test("the badge counts what is unread and clears when it is read", () => {
  const list = merge(items(alert("a"), alert("c", { read: true })), [fromNote(note("b", "asks"))]);
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
  const many = items(...Array.from({ length: 12 }, (_, i) => alert(String(i))));
  assert.equal(badge(many), "9+");
});

test("read and dismiss go to the side that keeps each row", () => {
  const list = merge(items(alert("a")), [fromNote(note("b", "conflict"))]);
  assert.deepEqual(bySide(list, ["a", "b", "gone"]), { server: ["a"], mesh: ["b", "gone"] });
});

test("dismissing leaves the others", () => {
  const list = items(alert("a"), alert("b"), alert("c"));
  assert.deepEqual(without(list, ["b"]).map(i => i.id), ["a", "c"]);
  assert.deepEqual(without(list, list.map(i => i.id)), []);
  assert.equal(without(list, ["gone"]).length, 3);
});

test("an opened alert is found while the list has it", () => {
  const list = items(alert("a"), alert("b"));
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
});

test("a row opens what it is about", () => {
  const session = { agent: { machine_id: "m1", id: "s1" } } as unknown as Note["target"];
  assert.equal(opens(fromAlert(alert("a"))), "alert");
  assert.equal(opens(fromNote(note("f", "finished", { target: session }))), "session");
  assert.equal(opens(fromNote(note("v", "move_failed", { target: session }))), "session");
  assert.equal(opens(fromNote(note("c", "conflict"))), "sync");
  assert.equal(opens(fromNote(note("m", "offline"))), "machines");
  // A temporary machine removed after it was gone a while.
  assert.equal(opens(fromNote(note("r", "removed"))), "machines");
  // A skill conflict opens in the pane.
  assert.equal(opens(fromNote(note("s", "skill_conflict"))), "skill");
  const run = { projects: { project: "acme/widget", run: { id: 7, host: "github.com" } } };
  assert.equal(opens(fromNote(note("r", "run", { target: run }))), "project");
});

test("a row says where it came from and when", () => {
  assert.equal(subLine(fromAlert(alert("a", { source: "Places" })), "4 min"), "Places · 4 min");
});

test("a skill conflict lists its copies and which are on this Mac", () => {
  const i = fromNote(note("skill:notes", "skill_conflict", { title: "Skill notes differs on box-a and box-b", text: "box-a ~/.claude/skills\nbox-b ~/.agents/skills", here: ["box-a ~/.claude/skills"] }));
  assert.equal(skillName(i), "notes");
  assert.deepEqual(copies(i), [
    { machine: "box-a", folder: "~/.claude/skills", here: true },
    { machine: "box-b", folder: "~/.agents/skills", here: false },
  ]);
  // A note saved before `here` existed has no copy on this Mac.
  assert.ok(copies(fromNote(note("o", "skill_conflict", { text: "box-a ~/x" }))).every(c => !c.here));
});
