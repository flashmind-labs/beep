import { answers, calls } from "./steer-env.ts";
import test from "node:test";
import assert from "node:assert/strict";
import { bindPromptStash } from "../src/parts/session/stash.tsx";
import { hasStashMessage, stashFile, type StashMessage } from "../src/stores/chat/stash-state.ts";

const draft = (): StashMessage => ({ text: "Unsent draft", images: [], paths: ["src/login.ts"], files: [{ name: "sample.bin", mime: "application/octet-stream", size: 3, data: "AAH/" }] });
const settle = () => new Promise(resolve => setImmediate(resolve));

function composer(project: string, prepare: () => Promise<() => void> = async () => () => {}) {
  const root = document.createElement("form");
  const field = document.createElement("textarea");
  field.value = "Unsent draft";
  root.inert = false;
  root.append(field); document.body.append(root);
  let cleared = false;
  bindPromptStash(field as unknown as HTMLTextAreaElement, {
    project: () => project, hasAttachments: () => true, capture: async () => draft(),
    clear: () => { cleared = true; field.value = ""; }, prepare,
  });
  calls.length = 0;
  return {
    field, root, cleared: () => cleared,
    async press(letter: string) { field.dispatchEvent(new window.KeyboardEvent("keydown", { key: letter, ctrlKey: true, bubbles: true })); await settle(); await settle(); },
  };
}

test("a failed disk write preserves the draft and its attachments", async () => {
  const c = composer("failed-save");
  Object.defineProperty(answers, "prompt_stash_put", { configurable: true, get() { throw new Error("Storage unavailable"); } });
  answers.prompt_stash_list = [];
  await c.press("s");
  assert.equal(c.cleared(), false);
  assert.equal(c.field.value, "Unsent draft");
  assert.equal(c.field.readOnly, false);
  assert.equal(c.root.inert, false);
  delete answers.prompt_stash_put;
});

test("a successful stash saves full attachment bytes before clearing", async () => {
  const c = composer("saved-draft");
  answers.prompt_stash_put = "saved-id";
  answers.prompt_stash_list = [];
  await c.press("s");
  const put = calls.find(call => call.cmd === "prompt_stash_put");
  assert.deepEqual(put?.args, { project: "saved-draft", message: draft() });
  assert.equal(c.cleared(), true);
});

test("attachment restoration failure leaves the original entry on disk", async () => {
  const c = composer("failed-restore", async () => { throw new Error("The destination is offline"); });
  answers.prompt_stash_list = [{ id: "entry", text: "Stashed", images: [], files: ["sample.bin"] }];
  answers.prompt_stash_read = draft();
  await c.press("p");
  assert.equal(calls.some(call => call.cmd === "prompt_stash_take"), false);
  assert.equal(c.cleared(), false);
  assert.equal(c.field.value, "Unsent draft");
});

test("pop swaps the complete current draft and only applies a prepared message", async () => {
  let applied = false;
  const c = composer("swap-draft", async () => () => { applied = true; });
  answers.prompt_stash_list = [{ id: "entry", text: "Stashed", images: [], files: [] }];
  answers.prompt_stash_read = { ...draft(), text: "Stashed" };
  answers.prompt_stash_take = answers.prompt_stash_read;
  await c.press("p");
  assert.deepEqual(calls.find(call => call.cmd === "prompt_stash_take")?.args, { project: "swap-draft", id: "entry", swap: draft() });
  assert.equal(applied, true);
});

test("files-only drafts are stashed and retain arbitrary binary bytes", async () => {
  const message = { ...draft(), text: "", paths: [] };
  assert.equal(hasStashMessage(message), true);
  const file = stashFile(message.files[0]);
  assert.deepEqual(new Uint8Array(await file.arrayBuffer()), new Uint8Array([0, 1, 255]));
  assert.throws(() => stashFile({ ...message.files[0], size: 4 }), /incomplete/);
});
