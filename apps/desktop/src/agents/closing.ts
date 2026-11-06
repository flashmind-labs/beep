// Closing a session from any list. Its rows say "closing" from the click on
// and read as ended once the machine says so (`sessions-changed`), before the
// next list read does.
import { invoke, listen } from "../ui/tauri.ts";
import { Closings } from "./closing-state.ts";

type Ref = { machine_id: string; id: string };

const closings = new Closings();
const listeners = new Set<() => void>();
const changed = () => { for (const f of listeners) f(); };

/// Calls `f` whenever a session starts closing or ends, for the lists to draw again.
export const onClosing = (f: () => void) => void listeners.add(f);

/// The status a row shows for `s`: "closing" or "ended" around a close, else its own.
export const shownStatus = (s: Ref & { status: string }) => closings.stateOf(s, Date.now()) ?? s.status;

/// What a Running row says in place of its time while `s` closes, or once it ended.
export const closingNote = (s: Ref) => closings.stateOf(s, Date.now());

/// Closes session `s`. It resolves once the machine took the close; a refusal
/// leaves the row as it was and rejects.
export async function closeSession(s: Ref & { agent: string }) {
  closings.start(s, Date.now());
  changed();
  try {
    await invoke("agent_steer", { machineId: s.machine_id, agent: s.agent, session: s.id, steer: "close" });
  } catch (e) {
    closings.refused(s);
    changed();
    throw e;
  }
}

listen<string>("sessions-changed", e => { if (closings.ended(e.payload, Date.now())) changed(); });
