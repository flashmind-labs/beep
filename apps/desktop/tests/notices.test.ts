import test from "node:test";
import assert from "node:assert/strict";
import { type Alert, badge, bellLabel, bellShown, find, ids, markRead, subLine, unreadCount, without } from "../src/center/notices-state.ts";

const alert = (id: string, over: Partial<Alert> = {}): Alert => ({ session_id: id, title: `Title ${id}`, source: "Apps", text: `Text ${id}`, read: false, at: 1_760_000_000_000, ...over });

test("the badge counts what is unread and clears when it is read", () => {
  const list = [alert("a"), alert("b"), alert("c", { read: true })];
  assert.equal(unreadCount(list), 2);
  assert.equal(badge(list), "2");
  assert.equal(bellLabel(list), "Notifications, 2 unread");
  const read = markRead(list, ["a", "b"]);
  assert.equal(unreadCount(read), 0);
  assert.equal(badge(read), "");
  assert.equal(bellLabel(read), "Notifications");
  // The list it came from is not changed.
  assert.equal(unreadCount(list), 2);
});

test("the badge stops counting at 9", () => {
  const many = Array.from({ length: 12 }, (_, i) => alert(String(i)));
  assert.equal(badge(many), "9+");
});

test("dismissing leaves the others, and Clear takes every id", () => {
  const list = [alert("a"), alert("b"), alert("c")];
  assert.deepEqual(without(list, ["b"])?.map(a => a.session_id), ["a", "c"]);
  assert.deepEqual(without(list, ids(list)), []);
  assert.deepEqual(without(list, ["gone"])?.length, 3);
});

test("an opened notice is found while the list has it", () => {
  const list = [alert("a"), alert("b")];
  assert.equal(find(list, "b")?.text, "Text b");
  assert.equal(find(without(list, ["b"]), "b"), null);
  assert.equal(find(list, null), null);
});

test("with no alerts to have the bell is hidden and nothing counts", () => {
  assert.equal(bellShown(null), false);
  assert.equal(unreadCount(null), 0);
  assert.equal(badge(null), "");
  assert.equal(markRead(null, ["a"]), null);
  assert.equal(without(null, ["a"]), null);
  assert.deepEqual(ids(null), []);
  assert.equal(find(null, "a"), null);
  // A server with alerts and none waiting still shows the bell.
  assert.equal(bellShown([]), true);
});

test("a row says where it came from and when", () => {
  assert.equal(subLine(alert("a", { source: "Places" }), "4 min"), "Places · 4 min");
});
