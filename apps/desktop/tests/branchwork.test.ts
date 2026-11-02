import test from "node:test";
import assert from "node:assert/strict";
import { worktreeMachines, worktreeName, defaultPlaces, placeChoices, worktreesToAsk, previewLine, outsideThreads, checkoutNote } from "../src/branchwork.ts";

const preview = {
  project: "tools-org/uploader",
  default_branch: "main",
  checkouts: [
    { machine_id: "m1", machine_name: "build-box", folder: "/srv/uploader", remote: "upstream" },
    { machine_id: "m2", machine_name: "mac-box", folder: "/srv/code/uploader", remote: "origin" },
    { machine_id: "m1", machine_name: "build-box", folder: "/srv/other", remote: "origin" },
  ],
  branches: [
    { name: "retry-flag", pr: 812, pr_title: "Add a retry flag", ahead: 2, worktree: "/srv/uploader/.worktrees/retry-flag", in_checkout: true },
    { name: "fix/windows-paths", pr: null, pr_title: "", ahead: 3, worktree: null, in_checkout: true },
    { name: "from-fork", pr: 900, pr_title: "", ahead: 0, worktree: null, in_checkout: false },
    { name: "main", pr: null, pr_title: "", ahead: 0, worktree: "/srv/uploader", in_checkout: true },
  ],
};

test("worktrees go on online machines, the one read on first", () => {
  const machines = [
    { id: "a", name: "zeta", online: true },
    { id: "b", name: "alpha", online: true },
    { id: "c", name: "off", online: false },
  ];
  assert.deepEqual(worktreeMachines(machines, "a"), [{ id: "a", name: "zeta" }, { id: "b", name: "alpha" }]);
  assert.deepEqual(worktreeMachines(null, "a"), []);
  assert.equal(worktreeName("fix/windows-paths"), ".worktrees/fix-windows-paths");
});

test("an import picks a worktree for branches with work and none yet", () => {
  assert.deepEqual(defaultPlaces(preview), { "fix/windows-paths": "m1", "from-fork": "m1" });
  assert.deepEqual(placeChoices(preview, []), [{ id: "m1", name: "build-box" }, { id: "m2", name: "mac-box" }]);
  const none = { ...preview, checkouts: [] };
  assert.deepEqual(defaultPlaces(none)["from-fork"], "");
  assert.deepEqual(placeChoices(none, []), [{ id: "", name: "Where it is imported" }]);
  const picks = { "fix/windows-paths": "m2", "from-fork": "", "retry-flag": null };
  assert.deepEqual(worktreesToAsk(preview, picks), [
    { branch: "fix/windows-paths", machine_id: "m2" },
    { branch: "from-fork", machine_id: null },
  ]);
  assert.deepEqual(worktreesToAsk(null, picks), []);
});

test("a preview branch says its PR, how far ahead and where it lives", () => {
  assert.equal(previewLine(preview.branches[0], "main"), "PR #812 Add a retry flag · 2 ahead · /srv/uploader/.worktrees/retry-flag");
  assert.equal(previewLine(preview.branches[2], "main"), "PR #900 · not in the checkout");
  assert.equal(previewLine(preview.branches[3], "main"), "default branch · /srv/uploader");
  assert.equal(checkoutNote(preview), "Found a checkout on build-box at /srv/uploader (through upstream) and 2 more.");
  assert.equal(checkoutNote({ checkouts: [] }), "No checkout on your online machines yet; importing clones it.");
});

test("outside_threads_skip_registered_repos", () => {
  const view = {
    items: [
      { host: "github.com", repo: "acme/widget", number: 1, updated_at: 5 },
      { host: "github.com", repo: "tools-org/uploader", number: 812, updated_at: 9 },
      { host: "github.com", repo: "big-org/parser", number: 2210, updated_at: 3 },
      { host: "github.com", repo: "tools-org/uploader", number: 40, updated_at: 4 },
    ],
  };
  const rows = outsideThreads(view, [{ name: "Acme/Widget" }]);
  assert.deepEqual(rows.map(r => [r.repo, r.items.map(i => i.number)]), [
    ["tools-org/uploader", [812, 40]],
    ["big-org/parser", [2210]],
  ]);
  assert.deepEqual(outsideThreads(null, null), []);
});
