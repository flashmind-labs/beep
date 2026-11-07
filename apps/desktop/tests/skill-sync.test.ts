import { answers, calls } from "./dom-env.ts";
import test from "node:test";
import assert from "node:assert/strict";
import { setImmediate } from "node:timers/promises";
import { mountSkillSync } from "../src/settings/skill-sync.ts";

const handlers = new WeakMap<Element, Map<string, () => void>>();
Object.defineProperty(HTMLElement.prototype, "addEventListener", {
  value(this: Element, type: string, fn: () => void) {
    const events = handlers.get(this) ?? new Map();
    events.set(type, fn);
    handlers.set(this, events);
  },
});
function click(el: Element) {
  const handler = handlers.get(el)?.get("click");
  assert.ok(handler);
  handler();
}

test("skill sync waits for a save and keeps the saved state when a write fails", async () => {
  answers.skill_sync_get = true;
  const host = document.createElement("div");
  mountSkillSync(host);
  await setImmediate();
  const button = () => host.querySelector<HTMLButtonElement>('[role="switch"]')!;
  assert.equal(button().getAttribute("aria-checked"), "true");
  assert.match(host.textContent!, /Keep your coding agent skills in sync/);

  let finish: () => void = () => {};
  answers.skill_sync_set = new Promise<void>(resolve => { finish = resolve; });
  click(button());
  assert.equal(button().disabled, true);
  assert.equal(button().getAttribute("aria-checked"), "true");
  assert.deepEqual(calls.at(-1), { cmd: "skill_sync_set", args: { enabled: false } });
  finish();
  await setImmediate();
  assert.equal(button().disabled, false);
  assert.equal(button().getAttribute("aria-checked"), "false");
  assert.match(host.textContent!, /Existing files stay in place/);

  answers.skill_sync_set = Promise.reject("Save failed <detail>");
  click(button());
  await setImmediate();
  assert.equal(button().disabled, false);
  assert.equal(button().getAttribute("aria-checked"), "false");
  assert.equal(host.querySelector('[role="alert"]')?.textContent, "Save failed <detail>");
  assert.equal(host.querySelector("detail"), null);

  answers.skill_sync_get = false;
  const reopened = document.createElement("div");
  mountSkillSync(reopened);
  await setImmediate();
  assert.equal(reopened.querySelector('[role="switch"]')?.getAttribute("aria-checked"), "false");
});

test("skill sync can retry a failed read without allowing a write before it loads", async () => {
  answers.skill_sync_get = Promise.reject("Could not read the setting");
  const host = document.createElement("div");
  mountSkillSync(host);
  await setImmediate();
  assert.equal(host.querySelector<HTMLButtonElement>('[role="switch"]')?.disabled, true);
  assert.match(host.textContent!, /Could not read the setting/);
  answers.skill_sync_get = true;
  click(host.querySelector("[data-retry]")!);
  await setImmediate();
  assert.equal(host.querySelector<HTMLButtonElement>('[role="switch"]')?.disabled, false);
  assert.equal(host.querySelector('[role="alert"]'), null);
});
