import { answers, calls } from "./dom-env.ts";
import { test } from "node:test";
import assert from "node:assert/strict";
import { syncthingImportChoice, type SyncthingFiles } from "../src/screens/project/syncthing.ts";

test("import preserves existing Syncthing rules and ignores stale folder lookups", async () => {
  const choice = syncthingImportChoice();
  answers.project_sync_get = false;
  const follow = (folder: string) => {
    let reply = (_files: SyncthingFiles) => {};
    answers.project_syncthing_files = new Promise<SyncthingFiles>(resolve => { reply = resolve; });
    const done = choice.follow("build-box", folder);
    return { reply, done };
  };

  const first = follow("/srv/projects/one");
  const second = follow("/srv/projects/two");
  second.reply({ available: true, enabled: false, label: "Current" });
  await second.done;
  first.reply({ available: true, enabled: true, label: "Previous" });
  await first.done;

  assert.deepEqual(choice.state.getState(), { available: true, checked: false, indeterminate: false, label: "Current" });
  assert.equal(choice.value(), null, "reading the setting must not change it during import");
  assert.deepEqual(calls.filter(call => call.cmd === "project_syncthing_files").map(call => call.args), [
    { machineId: "build-box", folder: "/srv/projects/one" },
    { machineId: "build-box", folder: "/srv/projects/two" },
  ]);
  choice.change(true);
  const third = follow("/srv/projects/three");
  third.reply({ available: true, enabled: false, label: "Next" });
  await third.done;

  assert.equal(choice.value(), true, "a chosen import preference must survive folder changes");
  assert.deepEqual(choice.state.getState(), { available: true, checked: true, indeterminate: false, label: "Next" });
});
