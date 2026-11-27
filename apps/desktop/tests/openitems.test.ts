import test from "node:test";
import assert from "node:assert/strict";
import { newestFirst, projectStart, projectsWithOpen, waitsOnYou } from "../src/stores/center/openitems-state.ts";
import type { GitItemRow } from "../src/stores/center/openitems-state.ts";

const item = (over: Partial<GitItemRow> = {}): GitItemRow => ({
  host: "github.com", repo: "acme/widget", number: 1, kind: "pull_request", title: "A change",
  url: "https://github.com/acme/widget/pull/1", updated_at: 1000, authored: false, assigned: false,
  review_requested: false, draft: false, review: "none", checks_failing: false, ...over,
});

test("an item waits on you when it asks for your review or your PR needs work", () => {
  assert.equal(waitsOnYou(item({ review_requested: true })), true);
  assert.equal(waitsOnYou(item({ authored: true, review: "changes_requested" })), true);
  assert.equal(waitsOnYou(item({ authored: true, checks_failing: true })), true);
  assert.equal(waitsOnYou(item({ authored: true, review: "approved" })), false);
  assert.equal(waitsOnYou(item({ kind: "issue", assigned: true })), false);
});

test("items group by project, waiting ones first, with counts", () => {
  const view = {
    hosts: [{ host: "github.com", forgejo: false }],
    items: [
      item({ number: 3, kind: "issue", url: "https://github.com/acme/widget/issues/3", updated_at: 5000 }),
      item({ number: 2, authored: true, updated_at: 3000 }),
      item({ repo: "tools-org/uploader", host: "git.example.com", number: 61, url: "https://git.example.com/tools-org/uploader/pulls/61", review_requested: true, updated_at: 2000 }),
    ],
  };
  const rows = projectsWithOpen(view, [{ name: "acme/widget", imported: true, machines: [] }]);
  assert.deepEqual(rows.map(r => r.repo), ["tools-org/uploader", "acme/widget"]);
  const [uploader, widget] = rows;
  assert.equal(uploader.waiting, 1);
  assert.equal(uploader.project, null);
  assert.equal(uploader.url, "https://git.example.com/tools-org/uploader");
  assert.equal(widget.prs, 1);
  assert.equal(widget.issues, 1);
  assert.equal(widget.last, 5000);
  assert.equal(widget.project!.name, "acme/widget");
  assert.deepEqual(widget.items.map(i => i.number), [3, 2]);
});

test("an agent starts in the project, on an online machine with its checkout when one has it", () => {
  const project = { name: "acme/widget", machines: [{ id: "m1", folder: "/srv/project/widget" }, { id: "m2", folder: "/path/to/work/widget" }] };
  const machines = [{ id: "m1", online: false }, { id: "m2", online: true }];
  assert.deepEqual(projectStart(project, machines), { machine: "m2", cwd: "/path/to/work/widget", places: [{ machine: "m1", cwd: "/srv/project/widget" }, { machine: "m2", cwd: "/path/to/work/widget" }], project: "acme/widget" });
  assert.deepEqual(projectStart(project, []), { machine: null, cwd: null, places: project.machines.map(m => ({ machine: m.id, cwd: m.folder })), project: "acme/widget" });
});

test("open items sort by the latest activity, whatever waits on the user", () => {
  const rows = [item({ updated_at: 100, review_requested: true }), item({ updated_at: 300 }), item({ updated_at: 200, review_requested: true })];
  assert.deepEqual(rows.sort(newestFirst).map(r => r.updated_at), [300, 200, 100]);
});
