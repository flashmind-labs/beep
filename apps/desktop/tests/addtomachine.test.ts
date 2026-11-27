import test from "node:test";
import assert from "node:assert/strict";
import { browseStart, copyLine, offers, shortName } from "../src/parts/project/addmachine-state.ts";
import { whereRows } from "../src/stores/project/projects-state.ts";
import type { MachineDetail, SyncDetail } from "../src/parts/project/filesync.ts";

const folder = (path: string, over = {}) => ({ path, parent: "/srv", exists: true, folders: [], more: 0, empty: false, origin: null, repos: [], ...over });

test("the picker offers the folder itself, or one in it named for the project", () => {
  const at = (path: string, over = {}, change = false) => offers(folder(path, over), "acme/widget", change, "/srv");
  assert.deepEqual(at("/srv"), [{ label: "Add in a new folder", path: "/srv/acme/widget" }]);
  assert.deepEqual(at("/srv/code"), [{ label: "Add in a new folder", path: "/srv/code/widget" }]);
  assert.deepEqual(at("/srv/new", { exists: false, empty: true }), [{ label: "Add here", path: "/srv/new" }]);
  assert.deepEqual(at("/srv/empty", { empty: true }, true), [{ label: "Use this folder", path: "/srv/empty" }]);
  assert.deepEqual(at("/srv/clone", { origin: "https://example.test/acme/widget.git" }), [{ label: "Use this checkout", path: "/srv/clone" }]);
  // Its files are there already, without `.git`: it goes over them, not in `widget/widget`.
  assert.deepEqual(at("/srv/code/widget"), [{ label: "Add here, keeping its files", path: "/srv/code/widget" }]);
  // The folder named for it is the place: nothing is offered around it.
  assert.deepEqual(at("/srv/code", { folders: ["other", "widget"] }), []);
  assert.deepEqual(at("/srv", { folders: ["widget"] }), []);
  assert.equal(shortName("local"), "local");
});

test("the picker opens where the folder is", () => {
  assert.equal(browseStart("/srv/code/widget"), "/srv/code");
  assert.equal(browseStart("/widget"), "/");
  assert.equal(browseStart("~/code/"), "~");
  assert.equal(browseStart(""), null);
});

const row = (id: string, m: Partial<MachineDetail>): MachineDetail => ({ machine_id: id, machine_name: id, folder: "/f", status: "in_sync", files: 0, bytes: 0, pending: 0, conflicts: [], error: null, hint: null, paused: false, progress: null, waiting: [], can_start_over: false, can_resolve: true, ...m });
const detail = (...machines: MachineDetail[]): SyncDetail => ({ project: "acme/widget", pinned: false, paused: false, machines });

test("the copy counts against what the other machine holds", () => {
  const line = copyLine(detail(row("to", { status: "syncing", files: 1204, pending: 10 }), row("from", { files: 1880 })), "to", "laptop");
  assert.equal(line.text, "Copying to laptop: 1,204 of 1,880 files");
  assert.equal(line.done, false);
});

test("while one large file comes, the bytes, rate and time left show", () => {
  const progress = { files_done: 0, files_total: 11, bytes_done: 12_300_000, bytes_total: 24_700_000, rate: 1_200_000, eta_secs: 10, file: null };
  const line = copyLine(detail(row("to", { status: "syncing", pending: 11, progress }), row("from", { files: 11 })), "to", "laptop");
  assert.equal(line.text, "Copying to laptop: 12 MB of 25 MB · 1.2 MB/s · under a minute");
  assert.equal(line.pct, 49);
  assert.equal(line.file, null);
});

test("the large file it waits on shows how much of it came", () => {
  const file = { path: ".git/objects/pack/pack-1a2b.pack", done: 12_300_000, size: 40_000_000 };
  const progress = { files_done: 250, files_total: 253, bytes_done: 90_000_000, bytes_total: 120_000_000, rate: 0, eta_secs: null, file };
  const line = copyLine(detail(row("to", { status: "syncing", pending: 3, progress }), row("from", { files: 253 })), "to", "laptop");
  assert.equal(line.file, "pack-1a2b.pack: 12 MB of 40 MB");
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

test("a mesh project's machine without it gets Add", () => {
  const none = (mesh: boolean) => whereRows(null, [machine("a", { folder: "/f", mesh }), machine("b", { mesh })], null).find(r => r.id === "b")!;
  assert.equal(none(true).add, true);
  assert.equal(none(false).add, false);
});

test("no Add from a copy Syncthing holds or from an offline machine, and the row names Syncthing", () => {
  const b = (a: object, m: object = {}) => whereRows(null, [machine("a", { folder: "/f", mesh: true, ...a }), machine("b", { mesh: true, ...m })], null).find(r => r.id === "b")!;
  assert.equal(b({}).add, true);
  assert.deepEqual([b({ syncthing: "share" }).add, b({ syncthing: "share" }).syncthing], [false, "share"]);
  assert.deepEqual([b({ online: false }).add, b({ online: false }).syncthing], [false, null]);
  assert.equal(b({}, { online: false }).kind, "offline");
});
