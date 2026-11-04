import { bar } from "./dom-env.ts";
import test from "node:test";
import assert from "node:assert/strict";
import { enqueue, popQueued } from "../src/chat/chat.ts";
import { queued, S } from "../src/bar/core.ts";

const SESSION = "00000000-0000-4000-8000-000000000002";

test("Up takes the last queued prompt back to edit, one at a time", () => {
  S.view = "chat";
  S.session = SESSION;
  bar.innerHTML = '<div class="dbody" id="body"></div><textarea id="ask"></textarea><div class="dhints"></div>';
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
