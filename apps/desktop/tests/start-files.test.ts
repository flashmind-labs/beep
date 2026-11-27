import { answers, calls } from "./bar-env.ts";
import test from "node:test";
import assert from "node:assert/strict";
import { UPLOAD_PART } from "../src/stores/agentpane/files-state.ts";
import { StartFiles } from "../src/stores/agents/start-files.ts";

globalThis.FileReader = class {
  result = "";
  onload = () => {};
  readAsDataURL(blob: Blob) {
    blob.arrayBuffer().then(bytes => {
      this.result = `data:application/octet-stream;base64,${Buffer.from(bytes).toString("base64")}`;
      this.onload();
    });
  }
} as unknown as typeof FileReader;

const uploads = () => calls.filter(c => c.cmd === "agent_tree_upload").map(c => c.args as Record<string, unknown>);

test("a start uploads a dropped video in chunks and mentions it from any worktree", async () => {
  calls.length = 0;
  answers.agent_tree_upload = "/cache/attachments/screen recording 2.mov";
  const files = new StartFiles();
  files.add([new File([new Uint8Array(UPLOAD_PART + 1)], "screen recording.mov", { type: "video/quicktime" })]);
  assert.equal(calls.length, 0, "the drop waits for a machine");
  const text = await files.prompt("Inspect this", "machine-one", "/srv/project");
  assert.equal(text, 'Inspect this\n\n@"/cache/attachments/screen recording 2.mov"');
  const parts = uploads();
  assert.equal(parts.length, 2);
  assert.equal(parts[0].path, "screen recording.mov");
  assert.equal(parts[1].path, "/cache/attachments/screen recording 2.mov");
  assert.equal(parts[1].offset, UPLOAD_PART);
  assert.equal(parts[0].machineId, "machine-one");
  assert.equal(parts[0].root, "/srv/project");
  const attachment = parts[0].attachment;
  assert.ok(attachment && typeof attachment === "object" && "agent" in attachment && "session" in attachment);
  assert.equal(attachment.agent, "draft");
  assert.match(String(attachment.session), /^[0-9a-f-]{36}$/);
  assert.deepEqual(parts[1].attachment, attachment);
});

test("an image-only start puts the request before the uploaded original", async () => {
  calls.length = 0;
  answers.agent_tree_upload = "/cache/attachments/screenshot.png";
  const files = new StartFiles();
  files.add([new File(["image"], "screenshot.png", { type: "image/png" })]);
  assert.equal(await files.prompt("", "machine-one", "/srv/project"), "Check this image.\n\n@/cache/attachments/screenshot.png");
});

test("retrying a start reuses completed files but changing machines uploads again", async () => {
  calls.length = 0;
  answers.agent_tree_upload = "/cache/attachments/clip.mp4";
  const files = new StartFiles();
  files.add([new File(["video"], "clip.mp4")]);
  const first = await files.prompt("", "machine-one", "/srv/project");
  assert.equal(first, "@/cache/attachments/clip.mp4");
  await files.prompt("Try again", "machine-one", "/srv/project");
  assert.equal(uploads().length, 1);
  await files.prompt("", "machine-two", "/srv/project");
  await files.prompt("", "machine-two", "/srv/other");
  assert.equal(uploads().length, 2);
});

test("large recordings and empty files upload in bounded parts", async () => {
  calls.length = 0;
  answers.agent_tree_upload = "/cache/attachments/large.mov";
  const files = new StartFiles();
  const size = 64 * 1024 * 1024 + 1;
  const file = new File([], "large.mov");
  Object.defineProperty(file, "size", { value: size });
  file.slice = (start = 0, end = size) => {
    assert.ok(end - start <= UPLOAD_PART);
    return new Blob([new Uint8Array(end - start)]);
  };
  files.add([file, new File([], "empty.txt")]);
  assert.equal(files.pending.length, 2);
  await files.prompt("", "machine-one", "/srv/project");
  const parts = uploads();
  assert.equal(parts.length, 34);
  assert.equal(parts[0].size, size);
  assert.equal(parts.at(-1)?.size, 0);
});

test("an unreadable drop prevents starting and stays available for removal", async () => {
  const files = new StartFiles();
  files.add([{ name: "folder", size: 1, slice() { throw new Error("unreadable"); } } as unknown as File]);
  await assert.rejects(files.prompt("Task", "machine-one", "/srv/project"), /could not be read/);
  assert.equal(files.pending.length, 1);
  assert.equal(files.pending[0].path, null);
});

test("a file uploads when attached and is deleted when removed", async () => {
  calls.length = 0;
  answers.agent_tree_upload = "/cache/attachments/notes.txt";
  answers.agent_discard_draft_attachment = null;
  const files = new StartFiles();
  files.moveTo("machine-one", "/srv/project");
  files.add([new File(["notes"], "notes.txt")]);
  await files.prompt("", "machine-one", "/srv/project");
  assert.equal(uploads().length, 1, "Start reuses the upload made on attach");
  files.remove(0);
  const deleted = calls.filter(c => c.cmd === "agent_discard_draft_attachment").map(c => c.args as Record<string, unknown>);
  assert.deepEqual(deleted, [{
    machineId: "machine-one",
    draft: (uploads()[0].attachment as { session: string }).session,
    path: "/cache/attachments/notes.txt",
  }]);
});

test("a started prompt keeps its uploads and an abandoned one deletes them", async () => {
  calls.length = 0;
  answers.agent_tree_upload = "/cache/attachments/a.txt";
  answers.agent_discard_draft_attachment = null;
  const files = new StartFiles();
  files.add([new File(["a"], "a.txt")]);
  await files.prompt("", "machine-one", "/srv/project");
  files.forget();
  files.add([new File(["a"], "a.txt")]);
  await files.prompt("", "machine-one", "/srv/project");
  files.discardAll();
  assert.equal(calls.filter(c => c.cmd === "agent_discard_draft_attachment").length, 1);
});
