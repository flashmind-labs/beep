import { answers, nativeEvent } from "./bar-env.ts";
import test from "node:test";
import assert from "node:assert/strict";
import { loadSessions, sessions } from "../src/stores/agents/sessions.ts";
import type { AgentSession } from "../src/stores/agents/agents-state.ts";

test("a title that arrives before the first list applies when the list lands", async () => {
  nativeEvent("agent-title-changed", { machine_id: "m1", agent: "claude", session: "a1", title: "Early" });
  answers.agent_sessions = [{ machine_id: "m1", machine_name: "build-box", id: "a1", agent: "claude", title: "Old", cwd: "/srv/project", status: "idle", external: false, question: null }] as AgentSession[];
  await loadSessions();
  assert.equal(sessions.find(s => s.id === "a1")?.title, "Early");
});
