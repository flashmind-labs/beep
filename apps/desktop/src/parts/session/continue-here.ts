/// Continue here: ends a session's process outside Beep so the session goes on
/// in Beep, after the turn running there ends.

import { createStore } from "zustand";
import { confirmSheet } from "../../ui/menu.tsx";
import { invoke } from "../../ui/tauri.ts";
import { errorText } from "../../stores/machines/machines-state.ts";
import { AG, type AgentRow, openOf, where } from "../../stores/agents/agents-state.ts";
import { agentKind } from "../../stores/agents/agent-kind.ts";
import { agentKey } from "../../stores/agents/conversation-state.ts";

/// Where Continue here stands for the session `key` names.
export type Continuing =
  | { kind: "none" }
  | { kind: "waiting"; key: string }
  | { kind: "closing"; key: string }
  | { kind: "failed"; key: string; message: string };

export const continuing = createStore<Continuing>(() => ({ kind: "none" }));

/// Closing the process while a turn runs would cut the turn off.
const turnRuns = (s: AgentRow) => s.status === "working";

/// The open session as it stands now, when it is still `s`.
const current = (s: AgentRow) => {
  const now = AG.page?.session;
  return now && agentKey(now) === agentKey(s) ? now : s;
};

/// Asks first: the terminal the session runs in loses it.
export function askContinueHere(s: AgentRow) {
  confirmSheet({
    title: "Continue here?",
    text: `${agentKind(s.agent).name} closes in the terminal it runs in, and the session goes on here.`,
    note: "If a turn is running, Beep waits for it to end.",
    action: "Continue here",
    run: async () => {
      const now = current(s);
      if (turnRuns(now)) continuing.setState({ kind: "waiting", key: agentKey(now) }, true);
      else await closeOutside(now);
    },
  });
}

export function cancelContinue() {
  continuing.setState({ kind: "none" }, true);
}

/// Called with each read of the open session: closes it once the awaited turn ended.
export function continueAfterTurn(s: AgentRow) {
  const state = continuing.getState();
  if (state.kind !== "waiting" || state.key !== agentKey(s) || turnRuns(s)) return;
  closeOutside(s).catch(() => { /* Shown by the button. */ });
}

/// Another session opened: what was asked for the last one no longer holds.
export function keepContinuing(s: AgentRow) {
  const state = continuing.getState();
  if (state.kind !== "none" && state.key !== agentKey(s)) cancelContinue();
}

async function closeOutside(s: AgentRow) {
  const key = agentKey(s);
  continuing.setState({ kind: "closing", key }, true);

  try {
    await invoke("agent_close_external", where(openOf(s)));
    // The session reads as ended once its follow sees the process exit; the field replaces the banner.
    if (continuing.getState().kind === "closing") cancelContinue();
  } catch (e) {
    const message = errorText(e);
    continuing.setState({ kind: "failed", key, message }, true);
    throw new Error(message);
  }
}
