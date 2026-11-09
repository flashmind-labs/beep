import { answers, calls } from "./dom-env.ts";
import test from "node:test";
import assert from "node:assert/strict";
import { invoke } from "../src/ui/tauri.ts";
import { createSkillSync } from "../src/settings/skill-sync.ts";

test("skill sync waits for a save and keeps saved state when a write fails", async () => {
  answers.skill_sync_get = true;
  const store = createSkillSync(invoke);
  await store.load();
  assert.equal(store.snapshot().enabled, true);
  let finish: () => void = () => {};
  answers.skill_sync_set = new Promise<void>(resolve => { finish = resolve; });
  const saving = store.change();
  assert.equal(store.snapshot().busy, true);
  assert.equal(store.snapshot().enabled, true);
  assert.deepEqual(calls.at(-1), { cmd: "skill_sync_set", args: { enabled: false } });
  finish(); await saving;
  assert.equal(store.snapshot().enabled, false);
  answers.skill_sync_set = Promise.reject("Save failed <detail>");
  await store.change();
  assert.equal(store.snapshot().busy, false);
  assert.equal(store.snapshot().enabled, false);
  assert.equal(store.snapshot().error, "Save failed <detail>");
});

test("skill sync retries failed reads and blocks writes before loading", async () => {
  answers.skill_sync_get = Promise.reject("Could not read the setting");
  const store = createSkillSync(invoke);
  await store.load();
  assert.equal(store.snapshot().enabled, null);
  await store.change();
  assert.equal(store.snapshot().error, "Could not read the setting");
  answers.skill_sync_get = true;
  await store.load();
  assert.deepEqual(store.snapshot(), { enabled: true, busy: false, error: "" });
  answers.skill_sync_get = Promise.reject("Read failed again");
  await store.load();
  assert.equal(store.snapshot().enabled, null);
  await store.change();
  assert.equal(store.snapshot().error, "Read failed again");
});
