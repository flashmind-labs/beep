import test from "node:test";
import assert from "node:assert/strict";
import { nativeSession, nativeUpdate, isNativeSession } from "../src/agents/native-session-state.ts";
import { subagent } from "../src/agents/agentlog.ts";
import { sessionOffers } from "../src/agents/sessionmenu-state.ts";
import type { AgentRow } from "../src/agents/agents-state.ts";

const parent: AgentRow = { machine_id: "m1", machine_name: "build-box", id: "parent", agent: "codex", title: "Parent task", cwd: "/srv/project", external: false, status: "working" };

test("native cards name an independently readable conversation without putting metadata in the result", () => {
  assert.deepEqual(subagent("[steps] 1\n[step completed] Read src/lib.rs\n[agent] Explorer\n[session] subagent:codex:thread-1\nFound the cause."), {
    count: 1, steps: [{ status: "completed", title: "Read src/lib.rs" }], agent: "Explorer", session: "subagent:codex:thread-1", result: "Found the cause.", model: null,
  });
});

test("native child rows use their own state and keep their parent's ancestry", () => {
  const child = nativeSession(parent, "subagent:codex:thread-1", "Check the fixtures", "completed")!;
  assert.equal(child.status, "idle");
  assert.notEqual(child.id, parent.id);
  assert.deepEqual(child.parent, { machine_id: "m1", id: "parent", key: "" });
  const updated = nativeUpdate(child, { ...child, title: null, parent: null, status: "ended" });
  assert.equal(updated.title, "Check the fixtures");
  assert.equal(updated.status, "idle");
  assert.deepEqual(updated.parent, child.parent);
  assert.equal(nativeSession(parent, "subagent:claude:parent:child", "Wrong harness", "completed"), null);
});

test("native sessions cannot be resumed or stopped as independent processes", () => {
  for (const status of ["working", "idle", "ended"]) {
    assert.deepEqual(sessionOffers({ id: "subagent:codex:thread-1", status, external: true, agent: "codex" }), { stop: false, close: false, resume: false, delete: false });
  }

  for (const id of ["subagent:codex:../thread", "subagent:claude:root:child:extra", "subagent:codex:"]) assert.equal(isNativeSession(id), false);
});
