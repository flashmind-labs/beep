import test from "node:test";
import assert from "node:assert/strict";
import type { RunningAgent, RunningRow } from "../src/home.ts";
import type { ProjectRow } from "../src/projects-state.ts";
import { START, importRoot, folderUnder, next, looksLikeRepo, machineChips, mergeProjects, filterProjects, progressText, doneText, cleanError, present, repoFromRemote, defaultFolder, folderNote, folderHelp, agentLabel, foundSessions, importPlan, lookingText, runningIn, pastSessions, nextSessions, openOnLabel, whereRows, whereCount, sideProjects, splitOnTop, overviewGroups, barSegments, countsLine, latestLine, workersLabel, overviewSub, groupPath, inGroupPath, groupOf, addGroupFolder, groupMatches, removeProjectSheet, REMOVE_PROJECT, folderOn, withSyncthing } from "../src/projects-state.ts";

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

test("Home lists imported projects last, or beside Machines when wide", async () => {
  const { homeRows, homeColumns } = await import("../src/home.ts");
  const home = { machines: [], running: [{}], needs_you: [], updates: [], coming_up: [], projects: [{}] };
  assert.deepEqual(homeRows(home), [["running"], ["projects"]]);
  assert.deepEqual(homeColumns(home), { left: ["running"], right: ["machines", "projects"] });
  assert.deepEqual(homeRows({ ...home, projects: null }), [["running"]]);
});

test("empty slots never reach the sheet as text", () => {
  const a = { n: 1 }, b = "text";
  assert.deepEqual(present([a, null, undefined, false, b, 0, ""]), [a, b, 0, ""]);
  assert.deepEqual(present(null), []);
});

test("the default folder follows the machine's OS and the repository", () => {
  assert.equal(defaultFolder("/srv/work/", "linux", "n0-computer/iroh"), "/srv/work/projects/n0-computer/iroh");
  assert.equal(defaultFolder("/Users/u", "macos", "https://github.com/org/app.git"), "/Users/u/Projects/org/app");
  assert.equal(defaultFolder("/Users/u", "macos", ""), "/Users/u/Projects/org/repo");
  assert.equal(defaultFolder(null, "linux", "org/app"), null);
});

test("a GitHub remote reads as org/repo, others as they are", () => {
  assert.equal(repoFromRemote("git@github.com:org/app.git"), "org/app");
  assert.equal(repoFromRemote("https://github.com/org/app"), "org/app");
  assert.equal(repoFromRemote("https://git.example.com/team/app.git"), "https://git.example.com/team/app.git");
});

const listing = (over = {}) => ({ path: "/w/code", parent: "/w", exists: true, folders: [], more: 0, empty: false, origin: null, ...over });

test("the note says what importing into a folder does", () => {
  assert.equal(folderNote(listing({ origin: "git@github.com:org/app.git" }), ""), "A checkout of org/app. It is imported in place, without cloning.");
  assert.match(folderNote(listing({ origin: "" }), "")!, /no origin remote/);
  assert.equal(folderNote(listing({ exists: false, empty: true }), "org/app"), "Not there yet: the clone makes it.");
  assert.equal(folderNote(listing({ empty: true }), "org/app"), "Empty: the clone goes here.");
  assert.equal(folderNote(listing(), "org/app"), "Has other files: it goes in /w/code/app.");
  assert.equal(folderNote(null, "org/app"), null);
});

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

test("the help line names the default only while it is the default", () => {
  const machines = [{ id: "a", name: "box" }];
  assert.match(folderHelp(START, machines), /^Beep picks a machine/);
  const picked = next(START, { type: "pick", machine: "a" });
  assert.match(folderHelp(picked, machines), /^The default on box\./);
  assert.doesNotMatch(folderHelp(picked, machines), /Projects/);
  assert.match(folderHelp(next(picked, { type: "choose", listing: listing() }), machines), /^Private repositories/);
});

const found = {
  project: "org/app",
  remote: "git@github.com:org/app.git",
  places: [
    { machine_id: "m1", machine_name: "build-box", folder: "/home/user/proj" },
    { machine_id: "m2", machine_name: "mac", folder: "/Users/u/proj" },
  ],
  agents: [{ agent: "claude", sessions: 3 }, { agent: "codex", sessions: 2 }],
};

test("a found row imports each place in place and counts its sessions", () => {
  assert.deepEqual(importPlan(found), [
    { repo: "git@github.com:org/app.git", machineId: "m1", folder: "/home/user/proj" },
    { repo: "git@github.com:org/app.git", machineId: "m2", folder: "/Users/u/proj" },
  ]);
  assert.equal(foundSessions(found), 5);
  assert.deepEqual(importPlan(null), []);
  assert.equal(agentLabel("claude"), "Claude Code");
  assert.equal(agentLabel("opencode"), "OpenCode");
  assert.equal(agentLabel("other"), "other");
});

test("the looking line names how many machines are online", () => {
  assert.match(lookingText([{ online: true }, { online: false }]), /on 1 machine…$/);
  assert.match(lookingText([{ online: true }, { online: true }]), /on 2 machines…$/);
  assert.match(lookingText([]), /on your machines…$/);
});

test("running agents in a project's folders show under Agents, not Sessions", () => {
  const project = { machines: [{ id: "m1", folder: "/home/user/proj" }] };
  const running = [agent("m1", "/home/user/proj/sub", 5), agent("m1", "/home/user/other", 5), agent("m2", "/home/user/proj", 5)];
  running[0].id = "s1";
  assert.deepEqual(runningIn(project, running).map(r => r.cwd), ["/home/user/proj/sub"]);
  const sessions = [
    { machine_id: "m1", id: "s0", last_activity: 1 },
    { machine_id: "m1", id: "s1", last_activity: 9 },
    { machine_id: "m1", id: "s2", last_activity: 4 },
  ];
  assert.deepEqual(pastSessions(sessions, runningIn(project, running)).map(s => s.id), ["s2", "s0"]);
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

test("open on names GitHub or the host", () => {
  assert.equal(openOnLabel("https://github.com/org/app"), "Open on GitHub");
  assert.equal(openOnLabel("https://git.example.com/org/app"), "Open on git.example.com");
  assert.equal(openOnLabel("not a url"), null);
});

test("where it lives lists every machine: a checkout, checking, clone here, or offline", () => {
  const machines = [
    { id: "mac", name: "MacBook Air", online: true, folder: "/Users/user/Projects/acme/widget", checking: false, checked_at: null },
    { id: "gpu", name: "gpu-box", online: true, folder: "/home/user/code/widget", checking: false, checked_at: 5 },
    { id: "build", name: "build-box", online: true, folder: null, checking: false, checked_at: 9 },
    { id: "mini", name: "mini-box", online: true, folder: null, checking: true, checked_at: null },
    { id: "old", name: "old-box", online: false, folder: null, checking: false, checked_at: null },
  ];
  const rows = whereRows({ machines: [] }, machines, "mac");
  assert.deepEqual(rows.map(r => r.kind), ["here", "here", "none", "checking", "offline"]);
  assert.equal(rows[0].mac, true);
  assert.equal(rows[1].mac, false);
  assert.equal(rows[2].text, "Not on build-box");
  assert.equal(rows[2].checked_at, 9);
  assert.equal(rows[3].text, "Checking…");
  assert.equal(rows[4].text, "Checks when old-box is online");
  assert.equal(whereCount(rows, true), "2 of 5 machines");
});

test("a folder Syncthing keeps in step says so on its row", () => {
  const machines = [
    { id: "m1", name: "build-box", online: true, folder: "/srv/work/app", checking: false, checked_at: 1 },
    { id: "m2", name: "laptop", online: true, folder: "/srv/app", checking: false, checked_at: 1 },
    { id: "m3", name: "gpu-box", online: true, folder: null, checking: false, checked_at: 1 },
  ];
  const marked = withSyncthing(machines, [
    { machine_id: "m1", folder: "/srv/work/app", label: "Work" },
    { machine_id: "m2", folder: "/srv/other", label: "Other" },
  ]);
  const rows = whereRows({ machines: [] }, marked, null);
  assert.deepEqual(rows.map(r => r.syncthing ?? null), ["Work", null, null]);
  assert.equal(withSyncthing(null, []), null);
});

test("an old server's page lists the imported folders", () => {
  const rows = whereRows({ machines: [{ id: "m1", name: "build-box", folder: "/srv/app" }] }, null, null);
  assert.deepEqual(rows, [{ kind: "here", id: "m1", name: "build-box", folder: "/srv/app", mac: false }]);
  assert.equal(whereCount(rows, false), "1 machine");
});

test("the sidebar groups projects by org and keeps each one's colour index", () => {
  const rows = sideProjects([{ name: "acme/widget" }, { name: "scratch" }, { name: "north/pines" }, { name: "acme/relay" }]);
  assert.deepEqual(rows.map(r => r.org ?? r.repo ?? r.name), ["acme", "widget", "relay", "north", "pines", "scratch"]);
  assert.deepEqual(rows.filter(r => !r.org).map(r => r.i), [0, 3, 2, 1]);
  assert.deepEqual(sideProjects(null), []);
});

test("pinned projects come out in pin order and keep their colour", () => {
  const rows = [{ name: "acme/relay" }, { name: "acme/widget" }, { name: "northwind/pines" }];
  const { onTop, rest } = splitOnTop(rows, ["northwind/pines", "acme/relay", "gone/repo"]);
  assert.deepEqual(onTop.map(p => [p.name, p.i]), [["northwind/pines", 2], ["acme/relay", 0]]);
  assert.deepEqual(rest.map(p => [p.name, p.i]), [["acme/widget", 1]]);
  assert.deepEqual(sideProjects(rest).map(p => p.org ?? [p.repo, p.i]), ["acme", ["widget", 1]]);
  assert.deepEqual(splitOnTop(null, null), { onTop: [], rest: [] });
});

test("the overview groups cards by org, pinned first, with their sums", () => {
  const rows = [{ name: "acme/widget", machines: [] }, { name: "acme/relay", machines: [] }, { name: "notes", machines: [] }, { name: "zed/pines", machines: [] }];
  const sums = [{ name: "acme/relay", workers: 1, tasks: { planned: 0, running: 1, blocked: 1, review: 0, done: 2 } }];
  const groups = overviewGroups(rows, sums, ["acme/relay"]);
  assert.deepEqual(groups.map(g => g.org), ["acme", "zed", null]);
  assert.deepEqual(groups[0].cards.map(c => [c.repo, c.i, c.pinned, !!c.sum]), [["relay", 1, true, true], ["widget", 0, false, false]]);
  assert.equal(overviewSub(groups), "4 projects in 2 orgs, 1 worker running");
  assert.equal(overviewSub(overviewGroups(rows.slice(2, 3), [], [])), "1 project, no workers running");
});

test("a card's bar, counts and status line", () => {
  const counts = { planned: 1, running: 1, blocked: 0, review: 2, done: 4 };
  assert.deepEqual(barSegments(counts).map(s => [s.state, s.share]), [["running", 12.5], ["review", 25], ["planned", 12.5], ["done", 50]]);
  assert.deepEqual(barSegments({ planned: 0, running: 0, blocked: 0, review: 0, done: 0 }), []);
  assert.deepEqual(countsLine(counts), ["4 open", "2 in review", "4 done"]);
  assert.equal(latestLine({ slug: "fix-clippy", title: "Fix clippy", state: "blocked" }), "fix-clippy is blocked");
  assert.equal(latestLine({ slug: "x", title: "Login copy", state: "review", pr_url: "https://example.com/pr/1" }), "Login copy: its PR waits for your review");
  assert.equal(latestLine(null), "No tasks yet");
  assert.deepEqual([0, 1, 2].map(workersLabel), ["idle", "1 worker", "2 workers"]);
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

test("the sidebar lists groups before orgs, their projects by full name", () => {
  const rows = [
    { name: "acme/widget", machines: [{ folder: "/Users/u/Projects/work/widget" }] },
    { name: "acme/relay", machines: [{ folder: "/srv/relay" }] },
    { name: "notes", machines: [{ folder: "/home/user/work/notes" }] },
    { name: "scratch", machines: [{ folder: "/srv/scratch" }] },
  ];
  const side = sideProjects(rows, [games, field]);
  assert.deepEqual(side.map(r => r.group?.name ?? r.org ?? [r.repo ?? r.name, r.i]),
    ["work", ["acme/widget", 0], ["notes", 2], "acme", ["relay", 1], ["scratch", 3]]);
  assert.deepEqual(sideProjects(rows).map(r => r.org ?? r.repo ?? r.name), ["acme", "widget", "relay", "notes", "scratch"]);
});

test("the overview shows every group first, then orgs, then the rest", () => {
  const rows = [
    { name: "acme/widget", machines: [{ folder: "~/work/widget" }] },
    { name: "acme/relay", machines: [{ folder: "/srv/relay" }] },
    { name: "notes", machines: [{ folder: "/srv/notes" }] },
  ];
  const sums = [{ name: "acme/widget", workers: 2, tasks: { planned: 0, running: 2, blocked: 0, review: 0, done: 0 } }];
  const groups = overviewGroups(rows, sums, [], [field, games]);
  assert.deepEqual(groups.map(g => g.group?.name ?? g.org), ["work", "games", "acme", null]);
  assert.deepEqual(groups[0].cards.map(c => [c.repo, c.i, !!c.sum]), [["acme/widget", 0, true]]);
  assert.deepEqual(groups[1].cards, []);
  assert.deepEqual(groups[2].cards.map(c => c.repo), ["relay"]);
  assert.equal(overviewSub(groups), "3 projects in 2 groups and 1 org, 2 workers running");
});

test("the group sheet keeps each folder once, as groups compare it", () => {
  assert.deepEqual(addGroupFolder([], "~/Projects/work"), { paths: ["work"], error: null });
  assert.deepEqual(addGroupFolder(["work"], "/home/user/work").error, "That folder is in the group already.");
  assert.match(addGroupFolder([], "~/Projects").error || "", /holds every project/);
  assert.match(addGroupFolder([], "/").error || "", /holds every project/);
  assert.deepEqual(addGroupFolder(["a"], "  "), { paths: ["a"], error: null });
  const rows = [{ machines: [{ folder: "/home/user/work/x" }] }, { machines: [{ folder: "/srv/y" }] }, { machines: null }];
  assert.equal(groupMatches(rows, ["work"]), 1);
  assert.equal(groupMatches(rows, ["work", "/srv"]), 2);
  assert.equal(groupMatches(null, ["x"]), 0);
});

test("the remove sheet names the project and says no files are deleted", () => {
  const s = removeProjectSheet("org/app");
  assert.equal(s.title, "Remove org/app?");
  assert.equal(s.action, "Remove project");
  assert.match(s.text, /stops syncing it and its workers/);
  assert.match(s.text, /deletes its chat and tasks/);
  assert.match(s.note, /nothing is deleted on any machine/);
  assert.equal(REMOVE_PROJECT, "Remove project…");
  for (const t of [s.title, s.text, s.note, s.action]) assert.doesNotMatch(t, /[—→⇒←]/);
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
