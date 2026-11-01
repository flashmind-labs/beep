import test from "node:test";
import assert from "node:assert/strict";
import { START, next, looksLikeRepo, machineChips, mergeProjects, filterProjects, progressText, doneText, cleanError } from "../dist/projects-state.js";

const imported = [
  { name: "org/app", url: "https://github.com/org/app", machine_id: "m1", machine_name: "build-box", folder: "/srv/work/projects/org/app" },
  { name: "org/app", url: "https://github.com/org/app", machine_id: "m2", machine_name: "mac", folder: "/Users/u/Projects/org/app" },
];
const agent = (machine_id, cwd, last_activity) => ({ kind: "agent", machine_id, machine_name: machine_id, cwd, started_at: 1, last_activity });

test("agents in an imported checkout count for it; others make their own project", () => {
  const rows = mergeProjects(imported, [
    agent("m1", "/srv/work/projects/org/app", 50),
    agent("m1", "/srv/work/projects/org/app/.worktrees/fix", 80),
    agent("m2", "/Users/u/code/notes", 30),
    { kind: "research", topic: "x" },
  ]);
  assert.deepEqual(rows.map(r => r.name), ["org/app", "notes"]);
  assert.equal(rows[0].agents, 2);
  assert.equal(rows[0].last, 80);
  assert.equal(rows[0].machines.length, 2);
  assert.equal(rows[1].imported, false);
  assert.deepEqual(rows[1].machines, [{ id: "m2", name: "m2", folder: "/Users/u/code/notes" }]);
});

test("an agent on another machine with the same folder name is not the imported one", () => {
  const rows = mergeProjects(imported.slice(0, 1), [agent("m9", "/home/x/app", 5)]);
  assert.deepEqual(rows.map(r => [r.name, r.agents]), [["org/app", 0], ["app", 1]]);
  assert.deepEqual(mergeProjects(null, null), []);
});

test("picking a project filters the page to it", () => {
  const rows = mergeProjects(imported, []);
  assert.equal(filterProjects(rows, "org/app").length, 1);
  assert.equal(filterProjects(rows, null), rows);
});

test("the field takes org/repo and URLs, nothing else", () => {
  for (const ok of ["n0-computer/iroh", " org/repo ", "https://github.com/org/repo", "https://git.example.com/team/app.git", "git@github.com:org/repo.git", "github.com/org/repo"]) {
    assert.ok(looksLikeRepo(ok), ok);
  }
  for (const bad of ["", "iroh", "a/b/c", "https://github.com/org", "org/re po"]) {
    assert.ok(!looksLikeRepo(bad), bad);
  }
});

test("chips are Auto then the machines online", () => {
  const chips = machineChips([{ id: "a", name: "box", online: true }, { id: "b", name: "mac", online: false }]);
  assert.deepEqual(chips, [{ id: null, label: "Auto" }, { id: "a", label: "box" }]);
});

test("the sheet clones, then lands on the project", () => {
  let s = next(START, { type: "submit" });
  assert.equal(s, START, "nothing to import yet");
  s = next(s, { type: "edit", repo: " n0-computer/iroh " });
  s = next(s, { type: "pick", machine: "a" });
  s = next(s, { type: "submit" });
  assert.equal(s.step, "cloning");
  assert.equal(s.repo, "n0-computer/iroh");
  assert.equal(progressText(s, [{ id: "a", name: "box" }]), "Cloning n0-computer/iroh onto box…");
  assert.equal(next(s, { type: "edit", repo: "x" }), s, "the field is locked while cloning");
  const project = { name: "n0-computer/iroh", machine_name: "box", folder: "/w/projects/n0-computer/iroh", cloned: true };
  s = next(s, { type: "done", project });
  assert.equal(s.step, "done");
  assert.equal(doneText(project), "Cloned on box in /w/projects/n0-computer/iroh");
  assert.equal(doneText({ ...project, cloned: false }), "Already there, now listed on box in /w/projects/n0-computer/iroh");
});

test("a refusal goes back to the form with the server's sentence", () => {
  let s = next(next(START, { type: "edit", repo: "org/private" }), { type: "submit" });
  s = next(s, { type: "failed", message: "bad request: Could not find org/private on GitHub." });
  assert.equal(s.step, "form");
  assert.equal(s.repo, "org/private");
  assert.equal(s.error, "Could not find org/private on GitHub.");
  assert.equal(next(s, { type: "edit", repo: "org/privat" }).error, null);
  assert.equal(cleanError(""), "The import failed.");
});

test("Home lists imported projects last, or beside Machines when wide", async () => {
  const { homeRows, homeColumns } = await import("../dist/home.js");
  const home = { machines: [], running: [{}], needs_you: [], updates: [], coming_up: [], projects: [{}] };
  assert.deepEqual(homeRows(home), [["running"], ["projects"]]);
  assert.deepEqual(homeColumns(home), { left: ["running"], right: ["machines", "projects"] });
  assert.deepEqual(homeRows({ ...home, projects: null }), [["running"]]);
});
