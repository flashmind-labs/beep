import { calls } from "./dom-env.ts";
import test from "node:test";
import assert from "node:assert/strict";
import { makeGitHistory } from "../src/review/history.ts";
import { topic, type MeshEvent } from "../src/ui/mesh-events.ts";
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
  const history = makeGitHistory({ invoke, back() {}, sessions: () => [], openSession() {}, reviewBranch() {}, canReview: () => false });
  t.after(() => history.stop());
  const panels = history.panels("example");
  document.body.append(panels.navigation, panels.detail);
  await settle();
  assert.equal(reads, 1);
  const reader = calls.find(call => call.cmd === "mesh_watch")!.args as { onEvent: { onmessage(event: MeshEvent): void } };
  const send = (sequence: number, root = "/srv/project") => reader.onEvent.onmessage({ id: { epoch: "example", sequence }, machine_id: "machine-a", topic: topic("files", root), resync: false });
  send(1, "/srv/another");
  t.mock.timers.tick(100);
  await settle();
  assert.equal(reads, 1, "unrelated roots do not reload Git");
  send(2);
  t.mock.timers.tick(100);
  await settle();
  assert.equal(reads, 2);
  t.mock.timers.tick(86_400_000);
  await settle();
  assert.equal(reads, 2, "idle history has no read timer");
  history.stop();
  panels.navigation.remove(); panels.detail.remove();
  send(3);
  t.mock.timers.tick(100);
  await settle();
  assert.equal(reads, 2);
});
