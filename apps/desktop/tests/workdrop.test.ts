import { answers, calls, nativeEvent } from "./bar-env.ts";
import test, { afterEach } from "node:test";
import assert from "node:assert/strict";
import { readFile } from "../src/agentpane/read-file.ts";
import { type FilePart, P } from "../src/agentpane/media.ts";
import { UPLOAD_PART } from "../src/agentpane/files-state.ts";
import { MN, MAX_ATTACH_BYTES, attachReads, uploadDropped, activateMentions, rememberMentionImages, resetMentions, currentMentions } from "../src/agents/workdrop.ts";

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

test("a dropped file is kept for the next message and nothing is uploaded", async () => {
  setup("/cache/never");
  await uploadDropped([new File([new Uint8Array(UPLOAD_PART + 1)], "clip.mp4", { type: "video/mp4" })]);
  assert.deepEqual(uploads(), []);
  const [key] = MN.paths;
  assert.match(key, /^attachment:\d+:clip\.mp4$/);
  const file = MN.files.get(key);
  assert.deepEqual([file?.name, file?.mime, file?.size], ["clip.mp4", "video/mp4", UPLOAD_PART + 1]);
  assert.equal(file?.data.length, Math.ceil((UPLOAD_PART + 1) / 3) * 4);
});

test("a file over the limit is refused with its name and the field keeps nothing of it", async () => {
  setup("/cache/never");
  const errors: string[] = [];
  MN.error = text => { errors.push(text); };
  const big = new File([], "huge.bin");
  Object.defineProperty(big, "size", { value: MAX_ATTACH_BYTES + 1 });
  await uploadDropped([big]);
  assert.deepEqual(MN.paths, []);
  assert.equal(MN.files.size, 0);
  assert.match(errors[0], /^huge\.bin is larger than 512 MiB/);
});

test("choosing the project root or another folder uploads at once and keeps it permanent", async () => {
  for (const folder of ["", "src"]) {
    const path = folder ? `${folder}/notes.txt` : "notes.txt";
    setup(path);
    await uploadDropped([new File(["notes"], "notes.txt")], folder);
    assert.equal(uploads()[0].path, path);
    assert.equal(uploads()[0].attachment, null);
    assert.deepEqual(MN.paths, [path]);
    assert.equal(MN.files.size, 0);
  }
});

test("a batch of attachments stays pending until every file is read", async () => {
  setup("/cache/never");
  const work = uploadDropped([new File(["first"], "first.txt"), new File(["second"], "second.txt")]);
  const [batch] = MN.pending;
  assert.ok(batch.local);
  assert.equal(attachReads().length, 1);
  await work;
  await batch.ready;
  assert.equal(MN.pending.size, 0);
  assert.equal(attachReads().length, 0);
  assert.equal(MN.paths.length, 2);
});

test("files read after navigation stay with their original session", async () => {
  setup("/cache/never");
  activateMentions(session);
  const owner = currentMentions();
  const work = uploadDropped([new File(["notes"], "notes.txt")]);
  rememberMentionImages();
  const other = { ...session, id: "session-two", machine_id: "machine-two" };
  P.session = other;
  activateMentions(other);
  await work;
  assert.deepEqual(MN.paths, []);
  assert.equal(MN.pending.size, 0);
  assert.equal(owner.reading, 0);
  assert.equal(owner.files.size, 1);
  P.session = session;
  activateMentions(session);
  assert.equal(MN.paths.length, 1);
});

function remoteParts(parts: FilePart[]) {
  calls.length = 0;
  let at = 0;
  Object.defineProperty(answers, "agent_file", { configurable: true, get() {
    const part = parts[at++];
    assert.ok(part, "the reader only asks for available chunks");
    return part;
  } });
}

const part = (size: number, text: string): FilePart => ({ size, mime: "application/pdf", data: btoa(text) });
const remotePath = "/srv/project/reports/result.pdf";
afterEach(() => { delete answers.agent_file; });

test("remote media parts keep their offsets and publish only the complete file", async () => {
  remoteParts([part(6, "abc"), part(6, "def")]);
  const progress: [number, number][] = [];
  const result = await readFile(session.machine_id, remotePath, session.id, (done, size) => progress.push([done, size]));

  assert.equal(new TextDecoder().decode(result.bytes), "abcdef");
  assert.equal(result.mime, "application/pdf");
  assert.deepEqual(progress, [[3, 6], [6, 6]]);
  assert.deepEqual(calls.filter(call => call.cmd === "agent_file").map(call => call.args), [
    { machineId: session.machine_id, path: remotePath, offset: 0, session: session.id },
    { machineId: session.machine_id, path: remotePath, offset: 3, session: session.id },
  ]);
});

test("an empty remote media file is complete", async () => {
  remoteParts([part(0, "")]);
  const result = await readFile(session.machine_id, remotePath, null, () => {});

  assert.equal(result.bytes.length, 0);
  assert.equal(result.mime, "application/pdf");
  assert.equal(calls.filter(call => call.cmd === "agent_file").length, 1);
});

test("changing sizes, early EOF and trailing bytes never publish partial media", async () => {
  for (const parts of [
    [part(6, "abc"), part(3, "")],
    [part(6, "abc"), part(7, "defg")],
    [part(6, "abc"), part(6, "")],
    [part(6, "abc"), part(6, "defg")],
    [part(1, "ab")],
  ]) {
    remoteParts(parts);
    let progressCalls = 0;
    await assert.rejects(
      readFile(session.machine_id, remotePath, session.id, () => { progressCalls++; }),
      /file changed/,
    );
    const before = progressCalls;
    nativeEvent("agent-file-progress", { machine_id: session.machine_id, path: remotePath, done: 6, size: 6 });
    assert.equal(progressCalls, before, "failed reads release their progress listener");
  }
});

test("remote file sizes must fit exact nonnegative JavaScript integers", async () => {
  for (const size of [-1, 0.5, Number.NaN, Number.POSITIVE_INFINITY, Number.MAX_SAFE_INTEGER + 1]) {
    remoteParts([part(size, "a")]);

    await assert.rejects(
      readFile(session.machine_id, remotePath, session.id, () => {}),
      /file size is out of range/,
    );
    assert.equal(calls.filter(call => call.cmd === "agent_file").length, 1);
  }
});
