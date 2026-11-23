// Starts a challenger on a report, and when it has finished, leaves what
// it found as drafts in the challenged session's review.
import { invoke } from "../ui/tauri.ts";
import { AG, AGENT_READ_WAIT_MS, type AgentPage, type AgentSession, type Started } from "./agents.ts";
import { challengePrompt, challenges, findingDraft, parseFindings, updateChallenges, type Challenge } from "./challenge-state.ts";
import { openingOf } from "./report-state.ts";
import { reviewKey, updateReview } from "./review-state.ts";
import { ensureSessions, sessions, sessionsChanged, sessionsRead } from "./sessions.ts";
import { openAgent, startedWith } from "./session.tsx";

/// Starts a fresh agent of the same kind, on the same machine and folder,
/// on the report at entry `at` of the open session.
export async function challengeReport(at: number) {
  const page = AG.page, report = page?.entries[at - page.earlier];
  if (!page || report?.kind !== "agent") return;

  const s = page.session, prompt = challengePrompt(report.text, s.cwd);
  const started = await invoke<Started>("agent_begin", { machineId: s.machine_id, agent: s.agent, cwd: s.cwd, prompt, images: [], model: null, effort: null, fast: null, checkout: null, assistant: false, account: null });
  startedWith(started.session, prompt, []);

  const challenger = { machine: started.session.machine_id, agent: started.session.agent, session: started.session.id };
  updateChallenges(reviewKey(s.machine_id, s.id), list => [...list, { entry: at, opening: openingOf(report.text), challenger, state: "running", findings: 0 }]);
}

/// Opens the challenger's session.
export async function openChallenger(c: Challenge) {
  const row = findSession(c) ?? (await ensureSessions(), findSession(c));
  if (row) openAgent(row);
}

const findSession = (c: Challenge): AgentSession | undefined =>
  sessions.find(s => s.machine_id === c.challenger.machine && s.id === c.challenger.session);

// ----- Collecting -----

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
  } catch (e) {
    console.warn("challenge:", e);
  } finally {
    collecting.delete(id);
  }
}
