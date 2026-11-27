import test from "node:test";
import assert from "node:assert/strict";
import { containerRow, runningRow, runningTrees, shownCommand, type RunningProcess } from "../src/stores/agentpane/running-state.ts";

const process: RunningProcess = {
  pid: 42, command: "npm run dev", started_at: new Date(2026, 0, 2, 9, 4).getTime() / 1000,
  ports: [3000, 8080], left_group: true, parent: 0, pages: [],
};
const alone = (p: RunningProcess) => ({ root: p, children: [] });

test("running rows show a padded local time and escaped group label", () => {
  assert.deepEqual(runningRow(alone(process)), {
    command: "npm run dev", detail: "since 09:04 · left the group", page: null,
    ports: [{ label: ":3000", port: 3000 }, { label: ":8080", port: 8080 }],
  });
});

test("invalid ports are dropped and group members need no group label", () => {
  const row = runningRow(alone({ ...process, left_group: false, ports: [3000, -1, 0, 65536, 1.5] }));
  assert.equal(row.detail, "since 09:04");
  assert.deepEqual(row.ports, [{ label: ":3000", port: 3000 }]);
});

test("children group under the furthest listed ancestor and lend it their ports", () => {
  const rows = [
    { ...process, pid: 1, parent: 9, ports: [] },
    { ...process, pid: 2, parent: 1, ports: [] },
    { ...process, pid: 3, parent: 2, ports: [5173] },
    { ...process, pid: 4, parent: 9, ports: [] },
  ];
  const trees = runningTrees(rows);
  assert.deepEqual(trees.map(t => [t.root.pid, t.children.map(c => c.pid)]), [[1, [2, 3]], [4, []]]);
  const row = runningRow(trees[0] ?? alone(process));
  assert.equal(row.detail, "since 09:04 · 2 more processes · left the group");
  assert.deepEqual(row.ports, [{ label: ":5173", port: 5173 }]);
});

test("the page is the port the command names among those that answered with one", () => {
  const server = { ...process, command: "npx wrangler dev --port 8788 --ip 127.0.0.1", ports: [9229, 8788, 41243], pages: [41243, 8788] };
  assert.deepEqual(runningRow(alone(server)).page, { label: ":8788", port: 8788 });
  assert.deepEqual(runningRow(alone(server)).ports.map(p => p.port), [9229, 41243]);
  assert.deepEqual(runningRow(alone({ ...server, command: "npm run dev" })).page?.port, 41243);
  // A named port that does not answer with a page is not offered.
  assert.equal(runningRow(alone({ ...server, pages: [] })).page, null);
});

test("a parent cycle still ends", () => {
  const trees = runningTrees([{ ...process, pid: 1, parent: 2 }, { ...process, pid: 2, parent: 1 }]);
  assert.equal(trees.flatMap(t => [t.root, ...t.children]).length, 2);
});

test("shown commands drop shell wrappers and long paths", () => {
  const claude = "/usr/bin/zsh -c source /home/u/.claude/shell-snapshots/s.sh 2>/dev/null || true && eval 'git commit -m '\\''fix'\\''' \\< /dev/null && pwd -P";
  assert.equal(shownCommand(claude), "git commit -m 'fix'");
  assert.equal(shownCommand("/usr/bin/zsh -c source /home/u/.claude/shell-snapshots/s.sh && eval 'npm run te"), "npm run te");
  assert.equal(shownCommand("/bin/bash -lc npm test"), "npm test");
  assert.equal(shownCommand("node /home/u/app/node_modules/.bin/vitest run"), "node vitest run");
  assert.equal(shownCommand("/usr/bin/python3 /Users/u/x.py"), "python3 ~/x.py");
});

test("a container row shows its image and page first", () => {
  const container = { id: "c0ffee0123456789", name: "", image: "nginx", started_at: process.started_at, ports: [5432, 8080], pages: [8080] };
  assert.deepEqual(containerRow(container), {
    name: "c0ffee012345", detail: "nginx · since 09:04", page: { label: ":8080", port: 8080 },
    ports: [{ label: ":5432", port: 5432 }],
  });
});
