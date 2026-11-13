import { answers, calls } from "./bar-env.ts";
import test from "node:test";
import assert from "node:assert/strict";
import { readFile } from "../src/agentpane/read.ts";
import { P } from "../src/agentpane/media.ts";
import { UPLOAD_PART } from "../src/agentpane/files-state.ts";
import { MN, uploadDropped, activateMentions, rememberMentionImages, resetMentions, currentMentions } from "../src/agents/workdrop.ts";

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

const session = { id: "session-one", agent: "codex", machine_id: "machine-one", machine_name: "build-box", cwd: "/srv/project" };
const uploads = () => calls.filter(c => c.cmd === "agent_tree_upload").map(c => c.args as Record<string, unknown>);

function setup(result: string) {
  resetMentions();
  P.session = session;
  Object.assign(MN, { paths: [], uploads: [], ask: null });
  MN.error = error => assert.fail(error);
  calls.length = 0;
  answers.agent_tree_upload = result;
}

test("default uploads use the session cache and continue with the returned host path", async () => {
  const path = "/cache/attachments/session-one/upload/clip.mp4";
  setup(path);
  await uploadDropped([new File([new Uint8Array(UPLOAD_PART + 1)], "clip.mp4")]);
  const parts = uploads();
  assert.equal(parts.length, 2);
  assert.equal(parts[0].path, "clip.mp4");
  assert.equal(parts[1].path, path);
  assert.equal(parts[1].offset, UPLOAD_PART);
  assert.deepEqual(parts[0].attachment, { agent: "codex", session: "session-one" });
  assert.deepEqual(MN.paths, [path]);
});

test("choosing the project root or another folder keeps uploads permanent", async () => {
  for (const folder of ["", "src"]) {
    const path = folder ? `${folder}/notes.txt` : "notes.txt";
    setup(path);
    await uploadDropped([new File(["notes"], "notes.txt")], folder);
    assert.equal(uploads()[0].path, path);
    assert.equal(uploads()[0].attachment, null);
    assert.deepEqual(MN.paths, [path]);
  }
});

test("a temporary attachment keeps its absolute path while browsing another worktree", async () => {
  const path = "/cache/attachments/session-one/upload/notes.txt";
  setup(path);
  await uploadDropped([new File(["notes"], "notes.txt")], null, "/srv/project/.worktrees/fix");
  assert.deepEqual(MN.paths, [path]);
});

test("a batch stays pending through every file until its attachments are ready", async () => {
  setup("/cache/first.txt");
  const gate = new Promise<string>(resolve => {
    Object.defineProperty(answers, "agent_tree_upload", { configurable: true, get() {
      return gate;
    } });
    queueMicrotask(() => resolve("/cache/first.txt"));
  });
  const errors: string[] = [];
  MN.error = text => { errors.push(text); };
  const work = uploadDropped([
    new File(["first"], "first.txt"),
    new File(["second"], "second.txt"),
  ]);
  const [batch] = MN.pending;
  assert.ok(batch);
  const path = await gate;
  assert.equal(path, "/cache/first.txt");
  assert.equal(MN.pending.size, 1);
  await work;
  await batch.ready;
  assert.equal(MN.pending.size, 0);
  assert.equal(batch.failed, false);
  assert.equal(errors.length, 0);
  Object.defineProperty(answers, "agent_tree_upload", { configurable: true, value: null, writable: true });
});


test("multipart uploads continue to their original session after navigation", async () => {
  const path = "/cache/attachments/session-one/upload/clip.mp4";
  setup(path);
  activateMentions(session);
  const owner = currentMentions();
  let release = (_path: string) => {};
  const gate = new Promise<string>(resolve => { release = resolve; });
  answers.agent_tree_upload = gate;
  const work = uploadDropped([new File([new Uint8Array(UPLOAD_PART + 1)], "clip.mp4"), new File(["notes"], "notes.txt")]);
  rememberMentionImages();
  const other = { ...session, id: "session-two", machine_id: "machine-two" };
  P.session = other;
  activateMentions(other);
  release(path);
  const uploaded = await work;
  assert.equal(uploaded.length, 2);
  assert.equal(uploads().length, 3);
  assert.ok(uploads().every(part => part.machineId === session.machine_id));
  assert.ok(uploads().every(part => (part.attachment as { session: string }).session === session.id));
  assert.deepEqual(MN.paths, []);
  assert.equal(MN.pending.size, 0);
  assert.equal(owner.uploads.length, 0);
  assert.equal(owner.reading, 0);
  assert.ok(owner.files.has(path));
  P.session = session;
  activateMentions(session);
  assert.deepEqual(MN.paths, [path]);
});


test("the uploading device plays its local video without reading it back from the node", async () => {
  const path = "/cache/attachments/session-one/upload/local-video.mov";
  setup(path);
  await uploadDropped([new File(["video bytes"], "local-video.mov", { type: "video/quicktime" })]);
  calls.length = 0;
  const result = await readFile(session.machine_id, path, session.id, () => {});
  assert.equal(new TextDecoder().decode(result.bytes), "video bytes");
  assert.equal(result.mime, "video/quicktime");
  assert.equal(calls.filter(call => call.cmd === "agent_file").length, 0);
});
