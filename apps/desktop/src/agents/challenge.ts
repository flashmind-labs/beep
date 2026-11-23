// Starts a challenger on a report, and when it has finished, leaves what
// it found as drafts in the challenged session's review.
import { invoke, listen } from "../ui/tauri.ts";
import type { AgentStarted } from "./starting-state.ts";
import { AG, AGENT_READ_WAIT_MS, type AgentPage, type AgentRow, type AgentSession, type Started } from "./agents.ts";
import type { Tree } from "../agentpane/worktree-state.ts";
import { closeSession } from "./closing.ts";
import { challengePrompt, challenges, findingDraft, parseFindings, updateChallenges, type Challenge, type ChallengeModel } from "./challenge-state.ts";
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
  updateChallenges(reviewKey(s.machine_id, s.id), list => [...list, { entry: at, opening: openingOf(report.text), challenger, state: "running", findings: 0 }]);
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
  updateChallenges(key, list => list.filter(d => d !== c));

  // Deleted once the close has ended it; a harness that keeps its sessions in a database keeps it.
  for (let tries = 0; tries < DELETE_TRIES; tries++) {
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

/// Statuses of a session whose turn is over.
const FINISHED = new Set(["idle", "ended"]);

const running = () => Object.entries(challenges.getState().by).flatMap(([key, list]) => list.filter(c => c.state === "running").map(c => ({ key, c })));

/// Reads the sessions again while a challenger runs, as they change.
sessionsChanged.add(() => { if (running().length) void ensureSessions().catch(() => {}); });
sessionsRead.add(() => { for (const { key, c } of running()) if (FINISHED.has(findSession(c)?.status ?? "")) void collect(key, c); });

const collecting = new Set<string>();

/// Reads the finished challenger's last reply and leaves its findings as
/// drafts on the report it challenged.
async function collect(key: string, c: Challenge) {
  const id = `${c.challenger.machine}:${c.challenger.session}`;
  if (collecting.has(id)) return;
  collecting.add(id);

  try {
    const page = await invoke<AgentPage>("agent_view", { machineId: c.challenger.machine, agent: c.challenger.agent, session: c.challenger.session, last: 20 }, AGENT_READ_WAIT_MS);
    const reply = [...page.entries].reverse().find(e => e.kind === "agent");
    if (!reply) return;

    const findings = parseFindings(reply.text);
    updateReview(key, review => {
      const first = Math.max(0, ...review.comments.map(d => d.id)) + 1;
      return { ...review, comments: [...review.comments, ...findings.map((f, i) => findingDraft(f, first + i, c.entry, c.opening))] };
    });
    updateChallenges(key, list => list.map(d => d === c ? { ...d, state: "done", findings: findings.length } : d));
    // Its session stays to read; closing it removes the copy it worked in.
    await closeSession({ machine_id: c.challenger.machine, id: c.challenger.session, agent: c.challenger.agent }).catch(e => console.warn("challenge close:", e));
  } catch (e) {
    console.warn("challenge:", e);
  } finally {
    collecting.delete(id);
  }
}
