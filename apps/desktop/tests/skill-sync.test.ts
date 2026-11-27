import { answers, calls } from "./dom-env.ts";
import test from "node:test";
import assert from "node:assert/strict";
import { invoke } from "../src/ui/tauri.ts";
import { createSyncSetting } from "../src/stores/settings/skill-sync.ts";

test("skill sync waits for a save and keeps saved state when a write fails", async () => {
  answers.skill_sync_get = true;
  const store = createSyncSetting(invoke, "skills");
  await store.load();
  assert.equal(store.state.getState().enabled, true);
  let finish: () => void = () => {};
  answers.skill_sync_set = new Promise<void>(resolve => { finish = resolve; });
  const saving = store.change();
  assert.equal(store.state.getState().busy, true);
  assert.equal(store.state.getState().enabled, true);
  assert.deepEqual(calls.at(-1), { cmd: "skill_sync_set", args: { enabled: false } });
  finish(); await saving;
  assert.equal(store.state.getState().enabled, false);
  answers.skill_sync_set = Promise.reject("Save failed <detail>");
  await store.change();
  assert.equal(store.state.getState().busy, false);
  assert.equal(store.state.getState().enabled, false);
  assert.equal(store.state.getState().error, "Save failed <detail>");
});

test("skill sync retries failed reads and blocks writes before loading", async () => {
  answers.skill_sync_get = Promise.reject("Could not read the setting");
  const store = createSyncSetting(invoke, "skills");
  await store.load();
  assert.equal(store.state.getState().enabled, null);
  await store.change();
  assert.equal(store.state.getState().error, "Could not read the setting");
  answers.skill_sync_get = true;
  await store.load();
  assert.deepEqual(store.state.getState(), { enabled: true, busy: false, error: "" });
  answers.skill_sync_get = Promise.reject("Read failed again");
  await store.load();
  assert.equal(store.state.getState().enabled, null);
  await store.change();
  assert.equal(store.state.getState().error, "Read failed again");
});


test("project sync has one master switch and resumes through the saved setting", async () => {
  answers.project_sync_get = true;
  answers.project_sync_set = null;
  const store = createSyncSetting(invoke, "projects");
  await store.load();
  assert.equal(store.state.getState().enabled, true);
  await store.change();
  assert.deepEqual(calls.findLast(call => call.cmd === "project_sync_set"), { cmd: "project_sync_set", args: { enabled: false } });
  assert.equal(store.state.getState().enabled, false);
  await store.change();
  assert.deepEqual(calls.findLast(call => call.cmd === "project_sync_set"), { cmd: "project_sync_set", args: { enabled: true } });
  assert.equal(store.state.getState().enabled, true);
});

test("project sync rereads the master preference after a partial write failure", async () => {
  answers.project_sync_get = true;
  const store = createSyncSetting(invoke, "projects");
  await store.load();
  answers.project_sync_get = false;
  answers.project_sync_set = Promise.reject("Some machines did not answer");
  await store.change();
  assert.deepEqual(store.state.getState(), { enabled: false, busy: false, error: "Some machines did not answer" });
});
