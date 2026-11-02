import test from "node:test";
import assert from "node:assert/strict";
import type { RunningAgent, RunningRow } from "../src/home.ts";
import type { ProjectRow } from "../src/projects-state.ts";
import { START, next, looksLikeRepo, machineChips, mergeProjects, filterProjects, progressText, doneText, cleanError, present, repoFromRemote, defaultFolder, folderNote, folderHelp, agentLabel, foundSessions, importPlan, lookingText, runningIn, pastSessions, nextSessions, openOnLabel, whereRows, whereCount, sideProjects, splitOnTop, overviewGroups, barSegments, countsLine, latestLine, workersLabel, overviewSub } from "../src/projects-state.ts";

const imported = [
  { name: "org/app", url: "https://github.com/org/app", machine_id: "m1", machine_name: "build-box", folder: "/srv/work/projects/org/app" },
  { name: "org/app", url: "https://github.com/org/app", machine_id: "m2", machine_name: "mac", folder: "/Users/u/Projects/org/app" },
];
const agent = (machine_id: string, cwd: string, last_activity: number) => ({ kind: "agent", machine_id, machine_name: machine_id, cwd, started_at: 1, last_activity }) as RunningAgent;

test("agents in an imported checkout count for it; others make their own project", () => {
  const rows = mergeProjects(imported, [
    agent("m1", "/srv/work/projects/org/app", 50),
    agent("m1", "/srv/work/projects/org/app/.worktrees/fix", 80),
    agent("m2", "/Users/u/code/notes", 30),
    { kind: "research", topic: "x" } as RunningRow,
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
