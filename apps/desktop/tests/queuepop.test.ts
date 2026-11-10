import { bar } from "./chat-dom-env.ts";
import test from "node:test";
import assert from "node:assert/strict";
import { enqueue, popQueued } from "../src/chat/queue.ts";
import { renderChat } from "../src/chat/chat.ts";
import { queued, S } from "../src/bar/core.ts";

globalThis.getComputedStyle = window.getComputedStyle.bind(window);

const SESSION = "00000000-0000-4000-8000-000000000002";

test("Up takes the last queued prompt back to edit, one at a time", () => {
  S.link = "linked";
  S.view = "chat";
  S.session = SESSION;
  renderChat();
  const ask = bar.querySelector("#ask") as unknown as HTMLTextAreaElement;
  enqueue("first");
  enqueue("second");
  // A stopped reply holds the queue.
  queued.get(SESSION)!.held = true;

  assert.equal(popQueued(), true);
  assert.equal(ask.value, "second");
  assert.deepEqual(queued.get(SESSION)?.list, ["first"]);
  assert.match(bar.querySelector(".dhints")!.textContent!, /edit queued/);

  ask.value = "";
  assert.equal(popQueued(), true);
  assert.equal(ask.value, "first");
  assert.equal(queued.has(SESSION), false);
  assert.equal(bar.querySelector("#queue"), null);
  assert.doesNotMatch(bar.querySelector(".dhints")!.textContent!, /edit queued/);
  assert.equal(popQueued(), false);
});
