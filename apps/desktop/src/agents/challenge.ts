// Starts a challenger on a report, and when it has finished, leaves what
// it found as drafts in the challenged session's review.
import { invoke, listen } from "../ui/tauri.ts";
import { isStarting, type AgentStarted } from "./starting-state.ts";
import { AG, AGENT_READ_WAIT_MS, type AgentPage, type AgentRow, type AgentSession, type Started } from "./agents.ts";
import type { Tree } from "../agentpane/worktree-state.ts";
import { closeSession } from "./closing.ts";
import { challengePrompt, challengeResult, challengeStep, challenges, findingDraft, updateChallenges, type Challenge, type ChallengeModel } from "./challenge-state.ts";
import { openingOf } from "./report-state.ts";
import { reviewKey, updateReview } from "./review-state.ts";
import { ensureSessions, loadSessions, sessions, sessionsChanged, sessionsRead } from "./sessions.ts";
import { openAgent, startedWith } from "./session.tsx";

/// Starts a fresh agent on `pick`, on the open session's machine, in a
/// throwaway copy of the work: the last worktree the session made, else
/// its folder. It is given the report at entry `at`.
export async function challengeReport(at: number, pick: ChallengeModel) {
  const page = AG.page, report = page?.entries[at - page.earlier];
  if (!page || report?.kind !== "agent") return;

  const s = page.session, work = await workFolder(s), prompt = challengePrompt(report.text, work);
  const started = await invoke<Started>("agent_begin", {
    machineId: s.machine_id, agent: pick.agent, cwd: work, prompt, images: [], model: pick.model.trim() || null, effort: pick.effort,
    fast: pick.agent === "codex" ? pick.fast : null, checkout: "copy_worktree", assistant: false, account: null,
  });
  startedWith(started.session, prompt, []);

  const challenger = { machine: started.session.machine_id, agent: started.session.agent, session: started.session.id };
  updateChallenges(reviewKey(s.machine_id, s.id), list => [...list, { entry: at, opening: openingOf(report.text), challenger, state: "running", findings: 0, error: "" }]);
}

/// Where session `s` did its work: the last worktree it made, else its folder.
async function workFolder(s: AgentRow) {
  const trees = await invoke<Tree[] | null>("agent_worktrees", { machineId: s.machine_id, folder: s.cwd, session: s.id }).catch(() => null);
  return trees?.filter(t => t.created).at(-1)?.path ?? s.cwd;
}

/// Stops the challenger and removes it: its session, and the copy it
/// worked in, which goes with the session's close.
export async function stopChallenger(key: string, c: Challenge) {
  const ref = { machine_id: c.challenger.machine, id: c.challenger.session, agent: c.challenger.agent };
  await closeSession(ref);
  updateChallenges(key, list => list.filter(d => !sameChallenger(d, c)));

  // Deleted once the close has ended it; a harness that keeps its sessions in
  // a database keeps it. A start its machine has not named is gone with the close.
  for (let tries = 0; !isStarting(ref.id) && tries < DELETE_TRIES; tries++) {
    try {
      await invoke("agent_delete", { machineId: ref.machine_id, agent: ref.agent, session: ref.id });
      break;
    } catch {
      await new Promise(resolve => setTimeout(resolve, DELETE_WAIT_MS));
    }
  }
  await loadSessions().catch(() => {});
}

const DELETE_TRIES = 5, DELETE_WAIT_MS = 1_000;

/// Opens the challenger's session.
export async function openChallenger(c: Challenge) {
  const row = findSession(c) ?? (await ensureSessions(), findSession(c));
  if (row) openAgent(row);
}

const findSession = (c: Challenge): AgentSession | undefined =>
  sessions.find(s => s.machine_id === c.challenger.machine && s.id === c.challenger.session);

// ----- Collecting -----

/// A challenger's start goes by a temporary id until its machine names the session.
listen<AgentStarted>("agent-started", ({ payload: { start, session } }) => {
  const named = { machine: session.machine_id, agent: session.agent, session: session.id };
  for (const [key, list] of Object.entries(challenges.getState().by)) {
    if (list.some(c => c.challenger.session === start)) updateChallenges(key, all => all.map(c => c.challenger.session === start ? { ...c, challenger: named } : c));
  }
});

const active = () => Object.entries(challenges.getState().by).flatMap(([key, list]) => list.filter(c => c.state === "running" || c.state === "asking").map(c => ({ key, c })));

/// Reads the sessions again while a challenger runs, as they change, and
/// moves each challenge on with its challenger's session.
sessionsChanged.add(() => { if (active().length) void ensureSessions().catch(() => {}); });
sessionsRead.add(() => {
  for (const { key, c } of active()) {
    const step = challengeStep(findSession(c)?.status ?? null, isStarting(c.challenger.session));
    if (step === "finished") void collect(key, c);
    else if (step === "gone") settle(key, c, { state: "failed", error: "Its session is gone." });
    else if (step !== c.state) settle(key, c, { state: step });
  }
});

/// Sets what `change` says on challenge `c` of `key`, as it is now.
function settle(key: string, c: Challenge, change: Partial<Challenge>) {
  updateChallenges(key, list => list.map(d => sameChallenger(d, c) ? { ...d, ...change } : d));
}

const sameChallenger = (a: Challenge, b: Challenge) => a.challenger.machine === b.challenger.machine && a.challenger.session === b.challenger.session;

const collecting = new Set<string>();

/// Reads the finished challenger's last reply: a report leaves its findings
/// as drafts on the report it challenged; anything else fails the challenge,
/// saying why. Either way the challenger is closed, which removes its copy;
/// its session stays to read.
async function collect(key: string, c: Challenge) {
  const id = `${c.challenger.machine}:${c.challenger.session}`;
  if (collecting.has(id)) return;
  collecting.add(id);

  try {
    const page = await invoke<AgentPage>("agent_view", { machineId: c.challenger.machine, agent: c.challenger.agent, session: c.challenger.session, last: 20 }, AGENT_READ_WAIT_MS);
    const result = challengeResult(lastReply(page), page.error);

    if (result.state === "done") {
      updateReview(key, review => {
        const first = Math.max(0, ...review.comments.map(d => d.id)) + 1;
        return { ...review, comments: [...review.comments, ...result.findings.map((f, i) => findingDraft(f, first + i, c.entry, c.opening))] };
      });
      settle(key, c, { state: "done", findings: result.findings.length });
    } else {
      settle(key, c, { state: "failed", error: result.error });
    }

    if (page.session.status !== "ended") await closeSession({ machine_id: c.challenger.machine, id: c.challenger.session, agent: c.challenger.agent }).catch(e => console.warn("challenge close:", e));
  } catch (e) {
    // Read again with the next change of the sessions.
    console.warn("challenge:", e);
  } finally {
    collecting.delete(id);
  }
}

/// The agent's reply to the last message it was sent, if it gave one.
function lastReply(page: AgentPage) {
  const after = page.entries.findLastIndex(e => e.kind === "user");
  return page.entries.slice(after + 1).findLast(e => e.kind === "agent")?.text ?? null;
}
