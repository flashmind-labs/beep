import { SessionPicks, type SessionPick } from "./agentmodels.ts";
import type { ContinuePick } from "./continue-state.ts";
import { createStore } from "zustand/vanilla";
import { signal } from "../ui/signal.ts";
import { sentText } from "./agentlog.ts";
import { disposeLogShell } from "./log-shell.tsx";
import { sessionKey } from "./conversation-state.ts";
import { type AgentReturn, S } from "../bar/core.ts";
import type { ChatBody } from "../ui/turn.ts";
import { type AgentQuestion, type AgentRow, type AgentPage, type OpenAgent, AG, AGENT_ACTIVE, canTakeOver, openModes } from "./agents-state.ts";
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

/// Whether the open session offers modes to switch between.
export function hasModes() {
  const s = AG.page?.session;
  return !!s && !s.external && s.status !== "ended" && (openModes()?.modes.length ?? 0) > 1;
}

/// Binds an approval action to the question and options shown to the user,
/// so an answer reaches only the question it was shown for.
export function questionKey(q: AgentQuestion, session: OpenAgent) {
  return JSON.stringify([session.machine_id, session.agent, session.session, q.id, q.text, q.detail, q.options.map(o => [o.id, o.name, o.allow])]);
}

/// The model and thinking an ended session resumes with, by session id.
export const resumePicks = new Map<string, SessionPick>();
/// The harness, model and thinking a chat goes on with at its next message,
/// by session id, when it was not its own harness. The one place it is kept.
export const continuePicks = new Map<string, ContinuePick>();

/// The open session's log changed: its page draws it again.
export const logStale = signal();

/// The session that should be open, how it was asked for, and null when none is (the empty view).
/// Every opener sets it through `openSession`; the page shows whatever it says. `asked` counts
/// the requests, so asking again for the session already open reaches the page as a change: it
/// then keeps its log, reads again, scrolls to the latest entry and revives an ended page.
export const openState = createStore<{ row: AgentRow | null; why: string; back: AgentReturn | undefined; adopt: boolean; asked: number }>(
  () => ({ row: null, why: "", back: undefined, adopt: false, asked: 0 }),
);

/// Makes `row` the session that should be open. With `adopt`, the empty view that started it
/// becomes the session in place.
export function openSession(row: AgentRow, { why = "", back, adopt = false }: { why?: string; back?: AgentReturn; adopt?: boolean } = {}) {
  openState.setState(s => ({ row, why, back, adopt, asked: s.asked + 1 }));
}

/// No session should be open: the empty view takes the page.
export function closeSession() {
  openState.setState({ row: null });
}


/// How many times the open session was to be followed again, by a send or a click. The reader
/// (session-follow.ts) follows it and reads when it is past what it last read; messages.ts sits
/// below the views the reader draws, so it marks the staleness instead of reading.
export const followStale = createStore<{ stale: number }>(() => ({ stale: 0 }));

/// Follows the open session again after showing it or sending a message.
export function resumeFollow() {
  AG.changed = Date.now();
  followStale.setState(s => ({ stale: s.stale + 1 }));
}

/// The frame around the session's log is stale: `staleFrame` marks it, and the page draws it again.
/// `inPlace` keeps the pane beside the log as it is.
export const frameStale = createStore<{ stale: number; inPlace: boolean }>(() => ({ stale: 0, inPlace: false }));

export function staleFrame(inPlace: boolean) {
  frameStale.setState(s => ({ stale: s.stale + 1, inPlace }));
}
