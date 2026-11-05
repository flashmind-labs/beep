import test from "node:test";
import assert from "node:assert/strict";
import { browseStart, copyLine, destination, shortName } from "../src/project/addmachine-state.ts";
import { whereRows } from "../src/project/projects-state.ts";
import type { MachineDetail, SyncDetail } from "../src/project/filesync.ts";

const folder = (path: string, exists: boolean, empty: boolean) => ({ path, parent: "/srv", exists, folders: [], more: 0, empty, origin: null });

test("an empty folder takes the project, any other holds it", () => {
  assert.equal(destination(folder("/srv/new", false, true), "acme/widget"), "/srv/new/widget");
  assert.equal(destination(folder("/srv/empty", true, true), "acme/widget"), "/srv/empty");
  assert.equal(destination(folder("/srv/code/", true, false), "acme/widget"), "/srv/code/widget");
  assert.equal(shortName("local"), "local");
});

test("the browser opens where the path would go", () => {
  assert.equal(browseStart("/srv/code/widget"), "/srv/code");
  assert.equal(browseStart("/widget"), "/");
  assert.equal(browseStart("~/code/"), "~");
  assert.equal(browseStart(""), null);
});

const row = (id: string, m: Partial<MachineDetail>): MachineDetail => ({ machine_id: id, machine_name: id, folder: "/f", status: "in_sync", files: 0, bytes: 0, pending: 0, conflicts: [], error: null, hint: null, paused: false, progress: null, waiting: [], can_resolve: true, ...m });
const detail = (...machines: MachineDetail[]): SyncDetail => ({ project: "acme/widget", pinned: false, paused: false, machines });

test("the copy counts against what the other machine holds", () => {
  const line = copyLine(detail(row("to", { status: "syncing", files: 1204, pending: 10 }), row("from", { files: 1880 })), "to", "laptop");
  assert.equal(line.text, "Copying to laptop: 1,204 of 1,880 files");
  assert.equal(line.done, false);
});

test("a machine that just joined and says it is in sync is not done until it has the files", () => {
  assert.equal(copyLine(detail(row("to", {}), row("from", { files: 9 })), "to", "laptop").done, false);
  assert.equal(copyLine(detail(row("to", { status: "scanning", files: 9 }), row("from", { files: 9 })), "to", "laptop").done, false);
  assert.equal(copyLine(detail(row("to", { files: 9 }), row("from", { files: 9 })), "to", "laptop").done, true);
});

test("with the other machine silent, the count it knows shows", () => {
  const offline = row("from", { status: "offline" });
  assert.equal(copyLine(detail(row("to", { files: 3, pending: 2 }), offline), "to", "laptop").text, "Copying to laptop: 3 of 5 files");
  assert.equal(copyLine(detail(row("to", {}), offline), "to", "laptop").text, "Copying to laptop…");
  assert.equal(copyLine(null, "to", "laptop").done, false);
});

test("what holds the copy up is said", () => {
  const held = row("from", { files: 9, error: "Already synced by Syncthing (folder x)" });
  assert.equal(copyLine(detail(row("to", {}), held), "to", "laptop").note, "Synced by Syncthing");
  assert.equal(copyLine({ ...detail(row("to", {})), paused: true }, "to", "laptop").note, "Sync is paused.");
  assert.equal(copyLine(detail(row("to", { error: "disk full" })), "to", "laptop").note, "disk full");
});

const machine = (id: string, m: object) => ({ id, name: id, online: true, folder: null, checking: false, checked_at: null, ...m });

test("a mesh project's machine without it gets Add, a server's gets Copy here", () => {
  const none = (mesh: boolean) => whereRows(null, [machine("a", { folder: "/f", mesh }), machine("b", { mesh })], null).find(r => r.id === "b")!;
  assert.deepEqual([none(true).add, none(true).copy], [true, false]);
  assert.deepEqual([none(false).add, none(false).copy], [false, true]);
});

test("no Add from a copy Syncthing holds or from an offline machine, and the row names Syncthing", () => {
  const b = (a: object, m: object = {}) => whereRows(null, [machine("a", { folder: "/f", mesh: true, ...a }), machine("b", { mesh: true, ...m })], null).find(r => r.id === "b")!;
  assert.equal(b({}).add, true);
  assert.deepEqual([b({ syncthing: "share" }).add, b({ syncthing: "share" }).syncthing], [false, "share"]);
  assert.deepEqual([b({ online: false }).add, b({ online: false }).syncthing], [false, null]);
  assert.equal(b({}, { online: false }).kind, "offline");
});
