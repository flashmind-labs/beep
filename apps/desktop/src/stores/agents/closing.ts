import { forgetView } from "../agentpane/view-memory.ts";
// Running sessions disappear as soon as closing starts. A refusal restores them.
import { invoke, listen } from "../../ui/tauri.ts";
import type { RunningAgent } from "./running-state.ts";
import { Closings, type Ref } from "./closing-state.ts";

const closings = new Closings();
const listeners = new Set<() => void>();
const changed = () => { for (const f of listeners) f(); };

/// Calls `f` whenever a session starts closing or ends, for the lists to draw again.
export const onClosing = (f: () => void) => void listeners.add(f);

/// The status a row shows for `s`: "closing" or "ended" around a close, else its own.
export const shownStatus = (s: Ref & { status: string }) => closings.stateOf(s, Date.now()) ?? s.status;

/// Hide stale running entries during close; confirmed ended rows belong in history.
export const sessionVisible = (s: Ref & { status: string }) => s.status === "ended" || closings.stateOf(s, Date.now()) === null;

/// The running sessions that are not closing.
export const visibleRunning = (rows: readonly RunningAgent[] | null | undefined) =>
  (rows || []).filter(sessionVisible);

/// What a Running row says in place of its time while `s` closes, or once it ended.
export const closingNote = (s: Ref) => closings.stateOf(s, Date.now());

/// Closes session `s`. It resolves once the machine took the close; a refusal
/// leaves the row as it was and rejects.
export async function closeSession(s: Ref & { agent: string }) {
  closings.start(s, Date.now());
  changed();

  try {
    await invoke("agent_steer", { machineId: s.machine_id, agent: s.agent, session: s.id, steer: "close" });
    forgetView(s.machine_id, s.id);
    void invoke("agent_files_forget", { session: s.id }).catch(() => {});
  } catch (e) {
    closings.clear(s);
    changed();
    throw e;
  }
}

/// Session `s` was resumed: its row shows again, whatever its close said.
export function reopened(s: Ref) {
  closings.clear(s);
  changed();
}

listen<string>("sessions-changed", e => { if (closings.ended(e.payload, Date.now())) changed(); });
