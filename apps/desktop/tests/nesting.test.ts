import test from "node:test";
import assert from "node:assert/strict";
import { childRow, nest, parentRow } from "../src/agents/nesting-state.ts";
import { subagent } from "../src/agents/agentlog.ts";
import type { AgentSession } from "../src/agents/agents.ts";

let at = 0;
const row = (id: string, over: Partial<AgentSession> = {}): AgentSession => ({
  machine_id: "m1", machine_name: "Build Box", id, agent: "claude", title: id, cwd: "/srv/project",
  status: "working", doing: null, started_at: ++at, last_activity: at, external: false, question: null, parent: null, ...over,
});
const under = (machine_id: string, id: string, key = "") => ({ machine_id, id, key });

test("a child sits right under its parent and not as a row of its own", () => {
  const rows = [
    row("a"),
    row("other"),
    row("kid", { machine_id: "m2", machine_name: "Mac Box", parent: under("m1", "a") }),
    row("grandkid", { machine_id: "m2", machine_name: "Mac Box", parent: under("m2", "kid") }),
  ];
  assert.deepEqual(nest(rows).map(n => [n.row.id, n.depth]), [["a", 0], ["kid", 1], ["grandkid", 2], ["other", 0]]);
});

test("a parent is found by the agent's own id or the id its node listed it under", () => {
  const rows = [row("short1"), row("kid", { parent: under("m1", "agent-uuid", "short1") })];
  assert.equal(parentRow(rows, rows[1])?.id, "short1");
  const after = [row("agent-uuid"), row("kid", { parent: under("m1", "agent-uuid", "short1") })];
  assert.equal(parentRow(after, after[1])?.id, "agent-uuid");
  assert.equal(parentRow(after, after[0]), undefined);
});

test("a child whose parent is not listed stays a top-level row", () => {
  const rows = [row("kid", { parent: under("m9", "gone") }), row("a")];
  assert.deepEqual(nest(rows).map(n => [n.row.id, n.depth]), [["kid", 0], ["a", 0]]);
});

test("rows that name each other as parents are all listed once", () => {
  const rows = [row("x", { parent: under("m1", "y") }), row("y", { parent: under("m1", "x") })];
  assert.equal(nest(rows).length, 2);
});

test("an id on a card finds the row of the machine it names", () => {
  const rows = [row("c1", { machine_name: "Mac Box", machine_id: "m2" }), row("c1")];
  assert.equal(childRow(rows, "c1@mac-box")?.machine_id, "m2");
  assert.equal(childRow(rows, "c1@build-box")?.machine_id, "m1");
  assert.equal(childRow(rows, "agent-id@mac-box", "c1")?.id, "c1");
  assert.equal(childRow(rows, "nope@mac-box"), undefined);
});

test("an agent another agent started reads as a card that links to its session", () => {
  const sub = subagent("[steps] 1\n[agent] codex on mac-box\n[child] a3f9c1@mac-box idle k1\nBuilt it.");
  assert.deepEqual(sub, { count: 1, steps: [], result: "Built it.", agent: "codex on mac-box", child: { id: "a3f9c1@mac-box", state: "idle", key: "k1" } });
  // What the child spent rides on its card, and an unpriced child says so.
  const spent = subagent("[steps] 1\n[agent] codex on mac-box\n[child] a3f9c1@mac-box idle k1\n[spend] 18400 0.0412\nBuilt it.");
  assert.deepEqual(spent?.child, { id: "a3f9c1@mac-box", state: "idle", key: "k1", tokens: 18400, cost: 0.0412 });
  assert.equal(spent?.result, "Built it.");
  assert.equal(subagent("[steps] 1\n[child] a@b idle k1\n[spend] 5 -\nok")?.child?.cost, null);
  // A plain subagent is as before.
  assert.deepEqual(subagent("[steps] 0\n[agent] Explore\nDone"), { count: 0, steps: [], result: "Done", agent: "Explore" });
});
