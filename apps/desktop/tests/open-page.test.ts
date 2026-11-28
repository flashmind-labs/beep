import test from "node:test";
import assert from "node:assert/strict";
import type { AgentPage, AgentRow } from "../src/stores/agents/agents-state.ts";
import { appendEntry, clearQuestion, currentPage, openPage, removeEntry, setPageSession } from "../src/stores/agents/open-page.ts";

const session = { machine_id: "m1", machine_name: "build-box", id: "s1", agent: "codex", cwd: "/srv/project", status: "idle", external: false, question: null } as AgentRow;
const held: AgentPage = { session, plan: [], entries: [{ kind: "agent", text: "hello" }], earlier: 0, error: null, queued: 0, queued_messages: [], turns: 0, usage: null };

test("the open page never changes the page it opened on", () => {
  openPage(session, held);
  const mine = { kind: "user", text: "sent" };
  appendEntry(mine);
  assert.equal(currentPage()?.entries.length, 2);
  removeEntry(mine);
  appendEntry(mine);
  setPageSession({ ...session, status: "working" });
  clearQuestion();
  assert.equal(currentPage()?.session.status, "working");
  assert.deepEqual(held.entries.map(en => en.text), ["hello"]);
  assert.equal(held.session.status, "idle");
});

test("reviving an ended session clears its ending error", () => {
  const ended = { ...session, status: "ended" as const };
  openPage(ended, { ...held, session: ended, error: "the agent exited" });
  setPageSession({ ...ended });
  assert.equal(currentPage()?.error, "the agent exited");

  setPageSession({ ...session, status: "starting" });
  assert.equal(currentPage()?.session.status, "starting");
  assert.equal(currentPage()?.error, null);
});
