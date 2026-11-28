import { answers, nativeEvent } from "./bar-env.ts";
import test from "node:test";
import assert from "node:assert/strict";
import { keepNativeSession, loadSessions, sessions, sessionsRead } from "../src/stores/agents/sessions.ts";
import type { AgentSession } from "../src/stores/agents/agents-state.ts";

const row = (id: string, title: string | null = null) => (
  { machine_id: "m1", machine_name: "build-box", id, agent: "claude", title, cwd: "/srv/project", status: "idle", external: false, question: null }
) as AgentSession;

test("a renamed kept session keeps its title when the list is pushed again", async () => {
  answers.agent_sessions = [row("a1")];
  await loadSessions();

  keepNativeSession({ ...row("n1", "Old"), external: true });
  nativeEvent("agent-title-changed", { machine_id: "m1", agent: "claude", session: "n1", title: "New" });
  assert.equal(sessions.find(s => s.id === "n1")?.title, "New");

  answers.agent_sessions = [row("a1"), row("a2")];
  await loadSessions();
  assert.equal(sessions.find(s => s.id === "n1")?.title, "New");
});

test("keeping a session rings the readers once", async () => {
  answers.agent_sessions = [row("a1")];
  await loadSessions();

  let rung = 0;
  const count = () => { rung++; };
  sessionsRead.add(count);
  keepNativeSession({ ...row("n2"), external: true });
  sessionsRead.delete(count);
  assert.equal(rung, 1);
  assert.ok(sessions.some(s => s.id === "n2"));
});

test("keeping a row identical to the kept one does not ring the readers", async () => {
  answers.agent_sessions = [row("a1")];
  await loadSessions();
  const kept = { ...row("n3"), external: true };
  keepNativeSession(kept);

  let rung = 0;
  const count = () => { rung++; };
  sessionsRead.add(count);
  keepNativeSession({ ...kept });
  sessionsRead.delete(count);
  assert.equal(rung, 0);
});
