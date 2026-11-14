import { answers, calls } from "./bar-env.ts";
import test from "node:test";
import assert from "node:assert/strict";
import { UPLOAD_PART } from "../src/agentpane/files-state.ts";
import { StartFiles } from "../src/agents/start-files.ts";

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
  answers.agent_tree_upload = ".beep/uploads/screen recording 2.mov";
  const files = new StartFiles();
  files.add([new File([new Uint8Array(UPLOAD_PART + 1)], "screen recording.mov", { type: "video/quicktime" })]);
  assert.equal(calls.length, 0, "the drop must wait for the final machine selection");
  const text = await files.prompt("Inspect this", "machine-one", "/srv/project", () => {});
  assert.equal(text, 'Inspect this\n\n@"/srv/project/.beep/uploads/screen recording 2.mov"');
  const parts = uploads();
  assert.equal(parts.length, 2);
  assert.equal(parts[0].path, ".beep/uploads/screen recording.mov");
  assert.equal(parts[1].path, ".beep/uploads/screen recording 2.mov");
  assert.equal(parts[1].offset, UPLOAD_PART);
  assert.equal(parts[0].machineId, "machine-one");
  assert.equal(parts[0].root, "/srv/project");
  assert.equal(parts[0].attachment, null);
});

test("retrying a start reuses completed files but changing destinations uploads again", async () => {
  calls.length = 0;
  answers.agent_tree_upload = ".beep/uploads/clip.mp4";
  const files = new StartFiles();
  files.add([new File(["video"], "clip.mp4")]);
  const first = await files.prompt("", "machine-one", "/srv/project", () => {});
  assert.equal(first, "@/srv/project/.beep/uploads/clip.mp4");
  await files.prompt("Try again", "machine-one", "/srv/project", () => {});
  assert.equal(uploads().length, 1);
  await files.prompt("", "machine-two", "/srv/project", () => {});
  await files.prompt("", "machine-two", "/srv/other", () => {});
  assert.equal(uploads().length, 3);
});

test("large recordings and empty files upload in bounded parts", async () => {
  calls.length = 0;
  answers.agent_tree_upload = ".beep/uploads/large.mov";
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
  await files.prompt("", "machine-one", "/srv/project", () => {});
  const parts = uploads();
  assert.equal(parts.length, 34);
  assert.equal(parts[0].size, size);
  assert.equal(parts.at(-1)?.size, 0);
});

test("an unreadable drop prevents starting and stays available for removal", async () => {
  const files = new StartFiles();
  files.add([{ name: "folder", size: 1, slice() { throw new Error("unreadable"); } } as unknown as File]);
  await assert.rejects(files.prompt("Task", "machine-one", "/srv/project", () => {}), /could not be read/);
  assert.equal(files.pending.length, 1);
  assert.equal(files.pending[0].uploaded, undefined);
});
