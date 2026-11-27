import { calls } from "./dom-env.ts";
import test from "node:test";
import assert from "node:assert/strict";
import { feed } from "../src/parts/session/session-follow.ts";
import { transcripts, transcriptKey } from "../src/stores/agents/transcript-cache-state.ts";
import type { AgentPage } from "../src/stores/agents/agents-state.ts";

const page = { session: { machine_id: "m1", machine_name: "build-box", id: "s1", agent: "codex", cwd: "/srv/project", status: "idle", external: false, question: null }, entries: [], earlier: 0, plan: [], error: null, queued: 0, turns: 0, usage: null } as AgentPage;
const follows = () => calls.filter(c => c.cmd === "agent_follow").map(c => c.args as { after: unknown });

test("a follow goes on from a cursor only when it has a fingerprint", () => {
  const o = { machine_id: "m1", agent: "codex", session: "s1" };
  calls.length = 0;
  transcripts.seed(transcriptKey(o), { page, cursor: { start: 0, next: 0, fingerprint: "" } });
  feed.follow(o);
  assert.equal(follows().at(-1)?.after, null);
  feed.clear();

  transcripts.delete(transcriptKey(o));
  const cursor = { start: 0, next: 3, fingerprint: "abc" };
  transcripts.seed(transcriptKey(o), { page, cursor });
  feed.follow(o);
  assert.deepEqual(follows().at(-1)?.after, cursor);
  feed.clear();
});
