import test from "node:test";
import assert from "node:assert/strict";
import type { ProjectRow, SessionRow } from "../src/stores/project/projects-state.ts";
import { plainPath } from "../src/parts/project/filesync.ts";
import { START, folderHelp, folderUnder, goesInText, hasCustomFolder, projectUnder, next, looksLikeRepo, progressText, doneText, cleanError, typedFolder } from "../src/stores/addproject/importproject-state.ts";
import { foundFor, mergeProjects, projectRunning, projectPastSessions, pastSessions, nextSessions, whereRows, whereCount, groupPath, inGroupPath, groupOf, folderOn, parseFolded, sideProjects, syncedBy, forgeLinks } from "../src/stores/project/projects-state.ts";
import { overviewGroups, overviewSub } from "../src/stores/project/overview-state.ts";
import type { RunningAgent } from "../src/stores/agents/running-state.ts";

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

const listing = (over = {}) => ({ path: "/w/code", parent: "/w", exists: true, folders: [], more: 0, empty: false, origin: null, repos: [], ...over });

test("choosing a checkout fills the field; another machine drops the choice", () => {
  let s = next(START, { type: "choose", listing: listing() });
  assert.equal(s, START, "Auto has no folder to choose");
  s = next(s, { type: "pick", machine: "a" });
  s = next(s, { type: "choose", listing: listing({ origin: "git@github.com:org/app.git" }) });
  assert.equal(s.repo, "org/app");
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
    { machine_id: "m1", id: "s0", status: "ended", last_activity: 1 },
    { machine_id: "m1", id: "s1", status: "ended", last_activity: 9 },
    { machine_id: "m1", id: "s2", status: "ended", last_activity: 4 },
  ];
  assert.deepEqual(pastSessions(sessions, [running[0]]).map(s => s.id), ["s2", "s0"]);
});

test("live project sessions move to Running before Home catches up", () => {
  const project = { machines: [{ id: "m1", name: "build-box", folder: "/srv/project" }] };
  const session = (id: string, status: string, more = {}): SessionRow => ({
    machine_id: "m1", machine_name: "build-box", id, agent: "codex", title: id,
    cwd: "/srv/project", status, doing: null, started_at: 1, last_activity: 2,
    external: false, question: null, assistant: false, persona: null, ...more,
  });
  const sessions = ["starting", "working", "running", "asking", "waiting", "idle", "ended"].map(status => session(status, status));
  assert.deepEqual(pastSessions(sessions, []).map(s => s.id), ["ended"], "live statuses stay out of history before Running loads");
  assert.deepEqual(pastSessions(sessions, null).map(s => s.id), ["ended"]);
  const running = projectRunning(project, [], sessions);
  assert.deepEqual(running.map(s => s.id), sessions.slice(0, -1).map(s => s.id));
  assert.deepEqual(pastSessions(sessions, running).map(s => s.id), ["ended"]);
  assert.ok(running.every(s => s.kind === "agent"));
  assert.deepEqual(projectRunning(project, [], sessions.map(s => ({ ...s, status: "ended" }))), [], "ended rows return to history");
  assert.equal(projectRunning(project, null, undefined).length, 0);
});

test("project Running keeps Home's live state and includes sessions in other worktrees", () => {
  const project = { machines: [{ id: "m1", name: "build-box", folder: "/srv/project" }] };
  const live: RunningAgent = {
    kind: "agent", machine_id: "m1", machine_name: "build-box", id: "s1", agent: "codex",
    cwd: "/srv/project/.worktrees/fix", status: "asking", title: "Current", doing: "Approval",
    started_at: 1, last_activity: 5, external: false,
  };
  const history: SessionRow[] = [
    { ...live, status: "running", title: "Old", question: null, assistant: false, persona: null },
    { ...live, id: "s2", cwd: "/srv/other-worktree", question: null, assistant: false, persona: null },
    { ...live, id: "ended", status: "ended", question: null, assistant: false, persona: null },
  ];
  const other = { ...live, machine_id: "m2" };
  const running = projectRunning(project, [live, other, { ...live, id: "outside", cwd: "/srv/project-other" }], history);
  assert.deepEqual(running.map(s => s.id), ["s1", "s2"]);
  assert.equal(running[0], live);
  assert.equal(history[0].title, "Old", "history stays unchanged");
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

test("a page whose machines are not read yet lists the imported folders", () => {
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

test("a project goes under its org in the folder, a local one by name", () => {
  assert.equal(projectUnder("~/", "acme/widget"), "~/acme/widget");
  assert.equal(projectUnder("/srv/code", "local/notes"), "/srv/code/notes");
  assert.equal(folderUnder("/srv/projects/", "n0-computer/iroh"), "/srv/projects/n0-computer/iroh");
  assert.equal(folderUnder("/srv/projects", "https://git.example.com/acme/relay.git"), "/srv/projects/acme/relay");
  assert.equal(folderUnder("/srv/projects", "nope"), null);
});

test("typing a Clone into path treats it as the target folder unless it is the default root", () => {
  const onBox = next(START, { type: "pick", machine: "a" });
  assert.equal(typedFolder(null, "~"), null);
  assert.equal(typedFolder("", "~"), null);
  assert.equal(typedFolder("~/", "~"), null);
  assert.equal(typedFolder("~/work/widget/", "~"), "~/work/widget");
  assert.equal(hasCustomFolder(onBox, null, "~"), false);
  assert.equal(hasCustomFolder(onBox, "~", "~"), false);
  assert.equal(hasCustomFolder(onBox, "~/work/widget", "~"), true);
  assert.equal(hasCustomFolder(onBox, "", "~"), true, "Default restores the root after clearing");
  assert.equal(goesInText("acme/widget", "~/work/widget", null), "Goes in ~/work/widget.");
  assert.equal(goesInText("acme/widget", null, "~"), "Goes in ~/acme/widget.");
  assert.match(folderHelp(onBox, [{ id: "a", name: "build-box" }], false), /^The default on build-box\./);
  assert.doesNotMatch(folderHelp(onBox, [{ id: "a", name: "build-box" }], true), /The default on/);
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

test("a repository's found checkout matches whatever the case of its name", () => {
  const row = { name: "Org/App", remote: "org/app", places: [], agents: [], last_activity: 0 };
  assert.equal(foundFor([row], "org/app"), row);
  assert.equal(foundFor([row], "org/other"), null);
  assert.equal(foundFor(null, "org/app"), null);
});

test("UNC paths lose their verbatim prefix and POSIX paths stay as written", () => {
  assert.equal(plainPath("\\\\?\\UNC\\nas\\share"), "\\\\nas\\share");
  assert.equal(plainPath("/Users/me/code"), "/Users/me/code");
});

test("the header has one button per forge, the project's own first, one per server", () => {
  const host = (host: string, kind: "github" | "forgejo", primary: boolean, url: string | null = null) =>
    ({ host, kind, repo: "acme/widget", primary, url });
  // Before the runs are read: the project's page alone.
  assert.deepEqual(forgeLinks("https://github.com/acme/widget", null), [{ label: "GitHub", url: "https://github.com/acme/widget" }]);
  assert.deepEqual(forgeLinks("http://forge.example.com:3000/acme/widget", []), [{ label: "Forgejo", url: "http://forge.example.com:3000/acme/widget" }]);
  // A Forgejo host keeps its scheme and port from its page.
  assert.deepEqual(
    forgeLinks("https://github.com/acme/widget", [
      host("forge.example.com:3000", "forgejo", false, "http://forge.example.com:3000/acme/widget"),
      host("github.com", "github", true),
    ]),
    [
      { label: "GitHub", url: "https://github.com/acme/widget" },
      { label: "Forgejo", url: "http://forge.example.com:3000/acme/widget" },
    ],
  );
  // The same server named twice, over another port, is one button.
  assert.deepEqual(
    forgeLinks(null, [
      host("forge.example.com:3000", "forgejo", true, "http://forge.example.com:3000/acme/widget"),
      host("forge.example.com", "forgejo", false, "https://forge.example.com/acme/widget"),
    ]).map(f => f.label),
    ["Forgejo"],
  );
  // A Forgejo host with no page (from a server) gets no button; two Forgejo servers are named.
  assert.deepEqual(forgeLinks(null, [host("forge.example.com", "forgejo", true)]), []);
  assert.deepEqual(
    forgeLinks(null, [
      host("forge.example.com", "forgejo", true, "https://forge.example.com/acme/widget"),
      host("git.example.org", "forgejo", false, "https://git.example.org/acme/widget"),
    ]).map(f => f.label),
    ["forge.example.com", "git.example.org"],
  );
});


test("overview cards keep local groups, pins and live agents", () => {
  const rows = [
    { name: "acme/widget", machines: [{ folder: "~/work/widget" }], agents: 2 },
    { name: "acme/relay", machines: [], agents: 1 },
    { name: "acme/pines", machines: [], agents: 0 },
    { name: "notes", machines: [], agents: 0 },
  ];

  const groups = overviewGroups(rows, ["acme/pines"], [field]);
  assert.deepEqual(groups.map(g => [g.group?.name ?? g.org, g.cards.map(c => c.name)]), [
    ["work", ["acme/widget"]],
    ["acme", ["acme/pines", "acme/relay"]],
    [null, ["notes"]],
  ]);
  assert.equal(groups[0].cards[0].repo, "acme/widget");
  assert.equal(groups[1].cards[0].pinned, true);
  assert.equal(overviewSub(groups), "4 projects in 1 group and 1 org, 3 agents running");
  assert.equal(overviewSub(overviewGroups<{ name: string; agents: number }>([{ name: "notes", agents: 0 }], [], [])), "1 project, no agents running");
});

test("project lists a moved conversation once with its target title and state", () => {
  const project = { machines: [{ id: "m1", name: "build-box", folder: "/srv/project" }, { id: "m2", name: "target-box", folder: "/srv/project" }] };
  const source: SessionRow = {
    machine_id: "m1", machine_name: "build-box", id: "conversation", agent: "codex",
    title: "Continue", cwd: "/srv/project", status: "idle", doing: null,
    question: null, started_at: 1, last_activity: 2, external: false, assistant: false, persona: null,
  };
  const target = { ...source, machine_id: "m2", title: "Diff improvements", status: "working", last_activity: 100 };
  const moves = [{ session: source.id, agent: source.agent, from: "m1", to: "m2", project: "org/app", at: 3 }];
  const running = projectRunning(project, [{ ...target, kind: "agent" }], [source], moves);
  assert.deepEqual(running.map(s => [s.machine_id, s.title, s.status, s.last_activity]), [["m2", "Diff improvements", "working", 100]]);
  assert.deepEqual(projectPastSessions([{ ...source, status: "ended" }], running, moves), []);
  assert.equal(projectRunning(project, [], [source], moves)[0].machine_id, "m1", "source remains until target lists");
  assert.equal(projectPastSessions([{ ...source, status: "ended" }], [], moves).length, 1);
});
