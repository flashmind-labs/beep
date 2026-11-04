import test from "node:test";
import assert from "node:assert/strict";
import type { MacControl } from "../src/center/home-state.ts";
import { byRecent, remotePath, runningKey, runningLine, thisMacRow } from "../src/center/home-state.ts";

test("Running now puts the session that moved last first", () => {
  const agent = (id: string, started_at: number, last_activity: number) => ({ kind: "agent" as const, machine_id: "m", id, started_at, last_activity });
  const rows = [agent("old", 10, 20), { kind: "research" as const, session: "r", started_at: 30 }, agent("new", 5, 40), agent("quiet", 25, 0)];
  assert.deepEqual(byRecent(rows).map(runningKey), ["agent:m:new", "research:r", "agent:m:quiet", "agent:m:old"]);
  assert.equal(runningKey(rows[0]), "agent:m:old", "the list it was given stays as it was");
});

test("a running agent shows its title, then agent, repo and machine", () => {
  const r = { kind: "agent" as const, agent: "claude", title: "Fix the build", cwd: "/srv/project/widget-fix/src", machine_id: "m1", machine_name: "build-box" };
  assert.deepEqual(runningLine(r), { title: "Fix the build", sub: "src · build-box" });
  const projects = [
    { name: "org/widget", machine_id: "m1", folder: "/srv/project/widget-fix" },
    { name: "org/other", machine_id: "m2", folder: "/srv/project/widget-fix/src" },
  ];
  assert.deepEqual(runningLine(r, projects), { title: "Fix the build", sub: "widget · build-box" });
  assert.deepEqual(runningLine({ ...r, title: null, cwd: "" }), { title: "Claude Code", sub: "build-box" });
});

test("a remote's path drops the host, the port and .git", () => {
  assert.equal(remotePath("ssh://git@git.example:2222/group/sub/repo.git"), "group/sub/repo");
  assert.equal(remotePath("https://github.com/org/repo/"), "org/repo");
  assert.equal(remotePath("https://github.com/org"), null);
  assert.equal(remotePath("/srv/project/repo"), null);
  assert.equal(remotePath(null), null);
});

test("This Mac gets a row until the server lists it", () => {
  const mac = (over: Partial<MacControl> = {}): Omit<MacControl, "permissions"> => ({ id: "mac-key", name: "Studio", enabled: true, status: "connecting", error: null, ...over });
  assert.equal(thisMacRow(mac(), [{ id: "mac-key" }]), null);
  assert.equal(thisMacRow(mac(), null), null);
  assert.equal(thisMacRow(null, []), null);
  assert.equal(thisMacRow(mac({ status: "unlinked" }), []), null);
  const off = thisMacRow(mac({ enabled: false, status: "off" }), [{ id: "other" }]);
  assert.equal(off!.state, "Off");
  assert.equal(off!.action, "Add");
  assert.equal(off!.name, "This Mac");
  assert.equal(off!.sub, "Studio");
  const failed = thisMacRow(mac({ status: "failed", error: "connect timed out" }), []);
  assert.equal(failed!.state, "connect timed out");
  assert.match(failed!.title, /connect timed out/);
  assert.equal(thisMacRow(mac({ status: "connected" }), [])!.state, "Connecting");
});
