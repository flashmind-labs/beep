import { SessionPicks } from "./agentmodels.ts";
import { sentText } from "./agentlog.ts";
import { disposeLogShell } from "./log-shell.tsx";
import { sessionKey } from "./conversation-state.ts";
import { S } from "../bar/core.ts";
import type { ChatBody } from "../ui/turn.ts";
import { type AgentRow, type AgentPage, type OpenAgent, AG, AGENT_ACTIVE, canTakeOver } from "./agents-state.ts";
import { setFirstMessage } from "./open-page.ts";
import { isNativeSession } from "./native-session-state.ts";
import { isStarting } from "./starting-state.ts";

// What the open session's page keeps and answers about itself, below the page that draws it.

/// Session `s` was started with `prompt` and `images`: its log shows them until its machine
/// logs the prompt, which it sends only once the model and thinking are set.
export function startedWith(s: AgentRow, prompt: string, images: readonly { name: string }[]) {
  setFirstMessage(s, sentText(prompt, images));
}

/// The logs drawn last, by session, to go back to as they were: switching
/// between them puts their turns back without drawing them again. Out of the
/// page they do no work; their clocks stop (`Since`).
/// Each keeps the page it was drawn from, older entries read while it showed included.
export interface KeptLog { body: ChatBody; page: AgentPage }
export const keptLogs = new Map<string, KeptLog>();
const KEPT_LOGS = 3;

export function keepLog(key: string, b: ChatBody, page: AgentPage) {
  const prior = keptLogs.get(key);
  if (prior && prior.body !== b) disposeLogShell(prior.body);
  keptLogs.delete(key);
  keptLogs.set(key, { body: b, page });

  for (const [old, log] of keptLogs) {
    if (keptLogs.size <= KEPT_LOGS) break;
    keptLogs.delete(old);
    disposeLogShell(log.body);
  }
}

/// Drops the kept log of a deleted session.
export function forgetLog(s: { machine_id: string; id: string }) {
  const key = sessionKey(s);
  const log = keptLogs.get(key);
  if (!log) return;
  keptLogs.delete(key);
  disposeLogShell(log.body);
}

/// The note under the log: the one place that sets `AG.note`.
export function setNote(text: string) { AG.note = text; }

/// What a failed read said, which the note repeats until a read succeeds.
export function setReadNote(text: string) { AG.readError = AG.note = text; }

export const isOpen = (o: OpenAgent | null) => S.view === "agent" && AG.open && o === AG.open;

/// Whether the open session's turn runs, so Enter steers and ⌘. stops.
export const turnRuns = () => {
  const s = AG.page?.session;
  return !!s && !s.external && !s.question && !isStarting(s.id) && AGENT_ACTIVE.has(s.status);
};

/// Whether the user stopped the open session's turn and it has not ended.
export const stoppingNow = () => {
  const s = AG.page?.session;
  return !!s && AG.stopping?.id === s.id && AGENT_ACTIVE.has(s.status);
};

/// The harnesses that switch models in a running session.
export const SWITCHES = new Set(["claude", "codex", "agy", "opencode", "pi", "flsh", "hermes"]);
/// The model and thinking switched to in each session from here, by session.
export const sessionPicks = new SessionPicks();

/// A session started outside Beep that only shows here: still running there,
/// or of an agent Beep cannot take over. A closed one continues from the field.
export function followsOnly(s: AgentRow) {
  return isNativeSession(s.id) || (s.external && !(s.status === "ended" && canTakeOver(s.agent)));
}
