import test from "node:test";
import assert from "node:assert/strict";
import type { ProjectRow } from "../src/project/projects-state.ts";
import { START, importRoot, folderUnder, next, looksLikeRepo, progressText, doneText, cleanError } from "../src/addproject/importproject-state.ts";
import { awaitsView, foundFor, mergeProjects, plainPath, pastSessions, nextSessions, whereRows, whereCount, groupPath, inGroupPath, groupOf, folderOn, parseFolded, sideProjects, syncedBy } from "../src/project/projects-state.ts";
import type { RunningAgent, RunningRow } from "../src/agents/running-state.ts";

const imported = [
  { name: "org/app", url: "https://github.com/org/app", machine_id: "m1", machine_name: "build-box", folder: "/srv/work/projects/org/app" },
  { name: "org/app", url: "https://github.com/org/app", machine_id: "m2", machine_name: "mac", folder: "/Users/u/Projects/org/app" },
];
const agent = (machine_id: string, cwd: string, last_activity: number) => ({ kind: "agent", machine_id, machine_name: machine_id, cwd, started_at: 1, last_activity }) as RunningAgent;

test("agents in an imported checkout count for it; others are no project", () => {
  const rows = mergeProjects(imported, [
    agent("m1", "/srv/work/projects/org/app", 50),
    agent("m1", "/srv/work/projects/org/app/.worktrees/fix", 80),
    agent("m2", "/Users/u/code/notes", 30),
    agent("m9", "/home/x/app", 5),
    { kind: "research", topic: "x" } as RunningRow,
  ]);
  assert.deepEqual(rows.map(r => r.name), ["org/app"]);
  assert.equal(rows[0].agents, 2);
  assert.equal(rows[0].last, 80);
  assert.equal(rows[0].machines.length, 2);
  assert.deepEqual(mergeProjects(null, null), []);
});

test("the field takes org/repo and URLs, nothing else", () => {
  for (const ok of ["n0-computer/iroh", " org/repo ", "https://github.com/org/repo", "https://git.example.com/team/app.git", "git@github.com:org/repo.git", "github.com/org/repo"]) {
    assert.ok(looksLikeRepo(ok), ok);
  }
  for (const bad of ["", "iroh", "a/b/c", "https://github.com/org", "org/re po"]) {
    assert.ok(!looksLikeRepo(bad), bad);
  }
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
  const project = { name: "n0-computer/iroh", machine_name: "box", folder: "/w/projects/n0-computer/iroh", cloned: true } as ProjectRow;
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

const listing = (over = {}) => ({ path: "/w/code", parent: "/w", exists: true, folders: [], more: 0, empty: false, origin: null, ...over });

test("choosing a checkout fills the field; another machine drops the choice", () => {
  let s = next(START, { type: "choose", listing: listing() });
  assert.equal(s, START, "Auto has no folder to choose");
  s = next(s, { type: "pick", machine: "a" });
  s = next(s, { type: "browse", listing: listing() });
  assert.equal(s.browsing!.path, "/w/code");
  s = next(s, { type: "choose", listing: listing({ origin: "git@github.com:org/app.git" }) });
  assert.equal(s.repo, "org/app");
  assert.equal(s.browsing, null);
  assert.equal(s.chosen!.path, "/w/code");
  s = next(s, { type: "submit" });
  assert.equal(progressText(s, [{ id: "a", name: "box" }]), "Importing org/app onto box…");
  s = next(next(START, { type: "pick", machine: "a" }), { type: "choose", listing: listing() });
  assert.equal(next(s, { type: "pick", machine: "a" }), s, "the same chip keeps it");
  assert.equal(next(s, { type: "pick", machine: "b" }).chosen, null);
  assert.equal(next(s, { type: "unchoose" }).chosen, null);
  assert.equal(next(s, { type: "oops", message: "bad request: box cannot open /root." }).error, "box cannot open /root.");
});

test("past sessions leave out the running ones, newest first", () => {
  const running = [agent("m1", "/home/user/proj/sub", 5)];
  running[0].id = "s1";
  const sessions = [
    { machine_id: "m1", id: "s0", last_activity: 1 },
    { machine_id: "m1", id: "s1", last_activity: 9 },
    { machine_id: "m1", id: "s2", last_activity: 4 },
  ];
  assert.deepEqual(pastSessions(sessions, [running[0]]).map(s => s.id), ["s2", "s0"]);
});

test("a project's sessions redraw only when a read brings a different list", () => {
  const shown = [{ machine_id: "m1", id: "s1", last_activity: 4 }];
  assert.deepEqual(nextSessions(undefined, shown), { list: shown, changed: true });
  assert.deepEqual(nextSessions(shown, structuredClone(shown)), { list: shown, changed: false });
  const newer = [{ machine_id: "m1", id: "s1", last_activity: 9 }];
  assert.deepEqual(nextSessions(shown, newer), { list: newer, changed: true });
  assert.deepEqual(nextSessions(shown, null), { list: shown, changed: false }, "a failed read keeps the list");
  assert.deepEqual(nextSessions(undefined, null), { list: [], changed: true });
});

test("an old server's page lists the imported folders", () => {
  const rows = whereRows({ machines: [{ id: "m1", name: "build-box", folder: "/srv/app" }] }, null, null);
  assert.deepEqual(rows, [{ kind: "here", id: "m1", name: "build-box", folder: "/srv/app", mac: false }]);
  assert.equal(whereCount(rows, false), "1 machine");
});

test("group folders leave out the home folder and a leading Projects", () => {
  for (const path of ["~/work", "~/Projects/work", "~/projects/work/", "/home/user/work",
    "/home/user/Projects/work", "/Users/someone/Projects/work", "C:\\Users\\someone\\Projects\\work", "  ~/work  "]) {
    assert.equal(groupPath(path), "work", path);
  }
  assert.equal(groupPath("~/Projects/acme/widget"), "acme/widget");
  assert.equal(groupPath("~/work/Projects/x"), "work/Projects/x");
  for (const path of ["~", "~/", "/home/user", "/Users/someone/", "~/Projects", "", null]) assert.equal(groupPath(path), "", String(path));
  assert.equal(groupPath("/srv/project/"), "/srv/project");
  assert.equal(groupPath("/"), "/");
});

test("a folder is in a group at or under its folder, on any machine", () => {
  assert.ok(inGroupPath("/home/user/work/relay", "work"));
  assert.ok(inGroupPath("/Users/someone/Projects/work", "work"));
  assert.ok(!inGroupPath("/home/user/works/relay", "work"));
  assert.ok(!inGroupPath("/home/user/x", ""));
  assert.ok(inGroupPath("/srv/project/app", "/srv/project"));
  assert.ok(!inGroupPath("/home/user/srv/project/app", "/srv/project"));
});

const field = { id: "g1", name: "work", paths: ["work"] };
const games = { id: "g2", name: "games", paths: ["work/games", "/srv/games"] };

test("a project goes in the group with the deepest folder holding it", () => {
  const at = (...folders: string[]) => ({ machines: folders.map(folder => ({ folder })) });
  assert.equal(groupOf(at("/home/user/work/relay"), [field, games]), field);
  assert.equal(groupOf(at("/Users/u/Projects/work/games/pines"), [field, games]), games);
  assert.equal(groupOf(at("/srv/work/x", "/srv/games/y"), [field, games]), games);
  assert.equal(groupOf(at("/home/user/other"), [field, games]), null);
  assert.equal(groupOf(at("/home/user/work"), null), null);
  const twin = { id: "g3", name: "twin", paths: ["work"] };
  assert.equal(groupOf(at("~/work/a"), [field, twin]), field, "the older group wins a tie");
});

test("a start form's folder follows the picked machine", () => {
  const places = [{ machine: "mac", cwd: "/Users/you/Projects/work/tool" }, { machine: "box", cwd: "/home/you/work/tool" }];
  assert.equal(folderOn("/Users/you/Projects/work/tool", "box", places), "/home/you/work/tool");
  assert.equal(folderOn("/Users/you/Projects/work/tool", "other", places), "~/Projects/work/tool");
  assert.equal(folderOn("/Users/you/Projects/work/tool", null, null), "~/Projects/work/tool");
  assert.equal(folderOn("/home/you/", "other", null), "~");
  assert.equal(folderOn("/srv/data", "other", null), "/srv/data");
  assert.equal(folderOn("", "other", null), "");
});

test("Where it lives keeps the same order whatever order machines arrive in", () => {
  const m = (id: string, name: string, folder: string | null) => ({ id, name, folder, online: true, checking: false });
  const a = [m("2", "devbox", "/srv/project"), m("1", "Laptop", null), m("3", "build-box", "/srv/project")];
  const names = (rows: { name: string }[]) => rows.map(r => r.name);
  assert.deepEqual(names(whereRows({ machines: [] }, a as any, null)), ["build-box", "devbox", "Laptop"]);
  assert.deepEqual(names(whereRows({ machines: [] }, [...a].reverse() as any, null)), ["build-box", "devbox", "Laptop"]);
});

test("the next import goes under the folder the last one went under", () => {
  assert.equal(importRoot("/srv/projects/acme/widget", "acme/widget"), "/srv/projects");
  assert.equal(importRoot("/srv/code/widget/", "acme/widget"), "/srv/code");
  assert.equal(folderUnder("/srv/projects/", "n0-computer/iroh"), "/srv/projects/n0-computer/iroh");
  assert.equal(folderUnder("/srv/projects", "https://git.example.com/acme/relay.git"), "/srv/projects/acme/relay");
  assert.equal(folderUnder("/srv/projects", "nope"), null);
});

test("a folded heading stands alone and names what it hides; loose projects stay", () => {
  const rows = [{ name: "acme/one" }, { name: "acme/two" }, { name: "other/three" }, { name: "loose" }, { name: "kept", machines: [{ folder: "/srv/work/kept" }] }];
  const groups = [{ id: "g1", name: "work", paths: ["/srv/work"] }];
  const names = (out: ReturnType<typeof sideProjects<(typeof rows)[number]>>) => out.map(p => p.group ? `group ${p.folded ?? ""}` : p.org ? `org ${p.org} ${p.folded ?? ""}` : p.name);
  assert.deepEqual(names(sideProjects(rows, groups)), ["group ", "kept", "org acme ", "acme/one", "acme/two", "org other ", "other/three", "loose"]);
  assert.deepEqual(names(sideProjects(rows, groups, new Set(["org:acme", "group:g1"]))), ["group kept", "org acme acme/one,acme/two", "org other ", "other/three", "loose"]);
});

test("the folded headings read back, and rubbish folds nothing", () => {
  assert.deepEqual([...parseFolded('["org:acme", 3, "group:g1"]')], ["org:acme", "group:g1"]);
  assert.equal(parseFolded("{").size, 0);
  assert.equal(parseFolded("{}").size, 0);
  assert.equal(parseFolded(null).size, 0);
});

test("a card names what keeps its project in step", () => {
  const rows = [{ project: "beep" }, { project: "both" }];
  assert.equal(syncedBy("beep", rows, [{ syncthing: null }]), "Beep sync");
  assert.equal(syncedBy("st", rows, [{}, { syncthing: "folder" }]), "Syncthing");
  assert.equal(syncedBy("both", rows, [{ syncthing: "folder" }]), "Beep sync · Syncthing");
  assert.equal(syncedBy("none", rows, [{ syncthing: null }]), "");
  assert.equal(syncedBy("none", rows, undefined), "");
  const held = [{ project: "st", error: "Already synced by Syncthing (Work)" }];
  assert.equal(syncedBy("st", held, [{ syncthing: "Work" }]), "Syncthing");
});

test("a project's page waits, not the agentless layout, until its agents are known", () => {
  const p = { name: "org/app", imported: true };
  assert.equal(awaitsView(p, undefined), true);
  assert.equal(awaitsView(p, [{ name: "org/app" }]), true);
  assert.equal(awaitsView(p, [{ name: "org/other" }]), false);
  assert.equal(awaitsView(p, null), false);
  assert.equal(awaitsView({ ...p, imported: false }, undefined), false);
});

test("a repository's found checkout matches whatever the case of its name", () => {
  const row = { name: "Org/App", remote: "org/app", places: [], agents: [], last_activity: 0 };
  assert.equal(foundFor([row], "org/app"), row);
  assert.equal(foundFor([row], "org/other"), null);
  assert.equal(foundFor(null, "org/app"), null);
});

test("a Windows folder shows without its verbatim prefix", () => {
  assert.equal(plainPath("\\\\?\\C:\\Users\\me\\games"), "C:\\Users\\me\\games");
  assert.equal(plainPath("\\\\?\\UNC\\nas\\share"), "\\\\nas\\share");
  assert.equal(plainPath("/Users/me/code"), "/Users/me/code");
});
