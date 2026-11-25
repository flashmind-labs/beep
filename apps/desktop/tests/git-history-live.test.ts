import { calls } from "./git-history-env.ts";
import test from "node:test";
import assert from "node:assert/strict";
import { makeGitHistory } from "../src/review/history.tsx";
import { topic, type MeshEvent } from "../src/ui/mesh-events.ts";
import { SLOW_MS } from "../src/ui/active.ts";
import type { HistoryPage } from "../src/review/history-state.ts";

const settle = () => new Promise<void>(done => setImmediate(done));

test("visible Git history follows its root and stops reading when closed", async t => {
  t.mock.timers.enable({ apis: ["setTimeout"] });
  Object.assign(document, { visibilityState: "visible" });
  const page: HistoryPage = { machine_id: "machine-a", machine_name: "build-box", root: "/srv/project", refs: [], worktrees: [], commits: [], next: null };
  let reads = 0;
  const invoke = (async (command: string) => {
    assert.equal(command, "project_git_history");
    reads++;
    return page;
  }) as Parameters<typeof makeGitHistory>[0]["invoke"];
  const history = makeGitHistory({ invoke, back() {}, sessions: () => [], openSession() {}, changedCheckout() {} });
  t.after(() => history.stop());
  const panels = history.panels("example");
  document.body.append(panels.navigation, panels.detail);
  await settle();
  assert.equal(reads, 1);
  const reader = calls.find(call => call.cmd === "mesh_watch")!.args as { onEvent: { onmessage(event: MeshEvent): void } };
  const send = (revision: number, root = "/srv/project", changed?: string[]) => reader.onEvent.onmessage({ revision, machine_id: "machine-a", topic: topic("files", root), data: changed ? { Sessions: { Tree: { changed } } } : {} });
  send(1, "/srv/another");
  t.mock.timers.tick(SLOW_MS);
  await settle();
  assert.equal(reads, 1, "unrelated roots do not reload Git");
  send(2);
  t.mock.timers.tick(SLOW_MS);
  await settle();
  assert.equal(reads, 2);
  t.mock.timers.tick(86_400_000);
  await settle();
  assert.equal(reads, 2, "idle history has no read timer");
  send(3, "/srv/project", [".git/FETCH_HEAD", ".git/config"]);
  await settle();
  assert.equal(reads, 2, "Git files nothing reads do not reload it");
  send(4, "/srv/project", [".git/refs/heads/main"]);
  await settle();
  assert.equal(reads, 3, "a moved ref reloads it at once, with no timer");
  send(5, "/srv/project", ["src/main.rs"]);
  await settle();
  assert.equal(reads, 4);
  history.stop();
  panels.navigation.remove(); panels.detail.remove();
  send(6);
  t.mock.timers.tick(SLOW_MS);
  await settle();
  assert.equal(reads, 4);
});

test("refresh keeps the selected commit diff mounted and preserves its scroll", async t => {
  Object.defineProperty(globalThis, "IntersectionObserver", { configurable: true, value: class { observe() {} unobserve() {} disconnect() {} } });
  t.after(() => Reflect.deleteProperty(globalThis, "IntersectionObserver"));
  const page: HistoryPage = { machine_id: "machine-a", machine_name: "build-box", root: "/srv/project", refs: [], worktrees: [],
    commits: [{ id: "abc123", parents: [], author: "Example", committed_at: 0, subject: "Change" }], next: null };
  let diffs = 0;
  const invoke = (async (command: string) => {
    if (command === "project_git_history") return page;
    if (command === "project_git_files") return [];
    if (command === "project_git_diff") {
      diffs++;
      return { root: "/srv/project", base: "", merge_base: "", working_tree: false, files: [], additions: 0, deletions: 0, more: 0 };
    }
    throw new Error(`Unexpected command: ${command}`);
  }) as Parameters<typeof makeGitHistory>[0]["invoke"];
  const history = makeGitHistory({ invoke, back() {}, sessions: () => [], openSession() {}, changedCheckout() {} });
  const panels = history.panels("example");
  document.body.append(panels.navigation, panels.detail);
  await settle();
  await settle();
  const files = panels.detail.querySelector<HTMLElement>(".dv-files");
  assert.ok(files);
  files.scrollTop = 200;
  const tree = panels.navigation.querySelector(".gh-tree")?.firstElementChild;
  await history.refresh("example");
  assert.equal(panels.detail.querySelector(".dv-files"), files);
  assert.equal(files.scrollTop, 200);
  assert.equal(panels.navigation.querySelector(".gh-tree")?.firstElementChild, tree);
  assert.equal(diffs, 1);
  history.stop();
  panels.navigation.remove(); panels.detail.remove();
});
