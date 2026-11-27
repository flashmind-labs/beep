import test from "node:test";
import assert from "node:assert/strict";
import { attachOf, heldMessage, outboxNote, stopPending } from "../src/stores/agents/agentqueue.ts";

test("a stop clears when its turn ends between reads and a steer starts another", () => {
  const stop = { id: "session", turns: 3 };
  assert.equal(stopPending(stop, "session", true, 3), stop);
  assert.equal(stopPending(stop, "session", true, 4), null);
  assert.equal(stopPending(stop, "session", false, 3), null);
  assert.equal(stopPending(stop, "other", true, 3), null);
});

test("a message the node holds shows its words and the names of its pictures", () => {
  assert.deepEqual(heldMessage("look at this\n[Image: shot.png]\n[Image: b.jpg]"), { words: "look at this", names: ["shot.png", "b.jpg"] });
  assert.deepEqual(heldMessage("[Image: shot.png]"), { words: "", names: ["shot.png"] });
  assert.deepEqual(heldMessage("plain\nwords"), { words: "plain\nwords", names: [] });
});

test("a sent row says what the machine does with it until its list shows it", () => {
  const row = { id: 1, text: "t", images: [], state: "sent" as const, reason: null, keys: [] };
  assert.equal(outboxNote({ ...row, how: "steer" }), "after the current step");
  assert.equal(outboxNote({ ...row, how: "prompt" }), "sent");
});

test("files go to the outbox with the paths the message mentions beside them", () => {
  const file = { name: "a.txt", mime: "text/plain", size: 1, data: "YQ==" };
  assert.equal(attachOf({ paths: ["src/lib.rs"], files: [] }), null);
  assert.deepEqual(attachOf({ paths: ["src/lib.rs", "attachment:1:a.txt"], files: [{ path: "attachment:1:a.txt", file }], root: "/srv/project" }), {
    root: "/srv/project", files: [{ name: "a.txt", mime: "text/plain", data: "YQ==" }], mentions: ["src/lib.rs"],
  });
});

test("an outbox row that uploads says so", () => {
  assert.equal(outboxNote({ id: 1, how: "prompt", text: "t", images: [], state: "uploading", reason: null, keys: [] }), "uploading");
});
