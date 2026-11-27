import { exitCode, parseTool } from "../agents/agentlog.ts";
import type { AgentEntry, AgentSession } from "../agents/agents-state.ts";
import { answersBefore, OLDER_PAGE, type Run } from "../agents/older-state.ts";
import { commandText, resultText } from "../agents/tool-details-state.ts";
import type { Entry } from "../../shell/center/palette-rank.ts";
import { invoke } from "../../ui/tauri.ts";

/// The search index supplies candidates. A PR URL in a prompt or a review
/// can find a candidate, but only the creation command and output link it.
async function findSession(url: string): Promise<AgentSession | null> {
  const hits = await invoke<Entry<AgentSession>[]>("center_find_prompts", { query: url });
  const candidates = hits.flatMap(hit => hit.session ? [hit.session] : []).slice(0, 8);
  for (const session of candidates) {
    if (await openedPr(session, url).catch(() => false)) return session;
  }
  return null;
}

async function openedPr({ machine_id, agent, id }: AgentSession, url: string) {
  let before: number | null = null;

  for (let page = 0; page < 4; page++) {
    const run: Run = await invoke<Run>("agent_older", { machineId: machine_id, agent, session: id, last: OLDER_PAGE, before });
    if (before !== null && !answersBefore(run, before)) return false;
    if (run.entries.some(entry => createdPr(entry, url))) return true;
    if (!run.earlier || !run.entries.length) return false;
    before = run.earlier;
  }

  return false;
}

const CREATE_PR = /(?:^|\n|&&|;)\s*(?:gh|tea|teacli)\s+(?:(?:-R|--repo)\s+\S+\s+)?(?:pr|pulls)\s+create(?:\s|$)/;
const SHELL_TITLE = /^(?:bash|shell|exec_command|shell_command):\s*/i;

function createdPr(entry: AgentEntry, url: string) {
  if (entry.kind !== "tool") return false;

  const { title, status, output } = parseTool(entry.text);
  const result = resultText(entry.tool);
  // Older Claude transcripts save the result on the call and mark only failures.
  if (status !== "completed" && (status !== "" || !result)) return false;

  const command = commandText(entry.tool) ?? (SHELL_TITLE.test(title) ? title.replace(SHELL_TITLE, "") : "");
  if (!CREATE_PR.test(command)) return false;
  if ((exitCode(output) ?? exitCode(result) ?? 0) !== 0) return false;

  return [result, output].some(text => text.split("\n").some(line => line.trim().replace(/^URL:\s*/, "") === url));
}

const RETRY_MS = 60_000;
type Lookup =
  | { kind: "checking"; listeners: Set<() => void> }
  | { kind: "checked"; session: AgentSession | null; at: number };

/// Shared PR-to-session lookup cache. Misses retry after `RETRY_MS` as recent
/// transcripts reach the search index or another machine reconnects; found
/// sessions stay cached and simultaneous completions coalesce redraws.
export function createPrSessionLookup(
  find: (url: string) => Promise<AgentSession | null> = findSession,
  now: () => number = () => Date.now(),
) {
  const lookups = new Map<string, Lookup>();
  const pendingRedraws = new Set<() => void>();
  let redrawScheduled = false;

  function scheduleRedraws(listeners: Iterable<() => void>) {
    for (const listener of listeners) pendingRedraws.add(listener);
    if (redrawScheduled) return;
    redrawScheduled = true;
    queueMicrotask(() => {
      redrawScheduled = false;
      const batch = [...pendingRedraws];
      pendingRedraws.clear();
      for (const listener of batch) listener();
    });
  }

  return function prSession(url: string, redraw: () => void): AgentSession | "checking" | null {
    const saved = lookups.get(url);
    if (saved?.kind === "checked" && (saved.session !== null || now() - saved.at < RETRY_MS)) return saved.session;
    if (saved?.kind === "checking") {
      saved.listeners.add(redraw);
      return "checking";
    }

    const listeners = new Set([redraw]);
    lookups.set(url, { kind: "checking", listeners });
    void find(url).catch(() => null).then(session => {
      lookups.set(url, { kind: "checked", session, at: now() });
      scheduleRedraws(listeners);
    });
    return "checking";
  };
}

/// One lookup shared by Home and the project page. A miss can be retried as
/// recent transcripts reach the search index or another machine reconnects.
export const prSession = createPrSessionLookup();
