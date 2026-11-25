// The session's mode: the pill by its field, Shift+Tab, and the plan waiting
// for approval above the field.
import { showAgentControl } from "./composer-ui.tsx";
import { AgentMode, AgentPlan } from "./mode-view.tsx";
import { type AgentModes, type OpenAgent, AG, openModes, AGENT_READ_WAIT_MS, openOf, where } from "./agents-state.ts";
import { agentKind } from "./agent-kind.ts";
import { questionKey, steerAgent } from "./messages.ts";
import { invoke } from "../ui/tauri.ts";
import { isStarting } from "./starting-state.ts";

/// Machines too old to know the request, so they are not asked again.
const oldMachines = new Set<string>();
/// A switch on its way; a press meanwhile is dropped.
let switching = false;

/// The modes of the open session; null when it has none to switch between.
export async function readModes(o: OpenAgent) {
  const s = AG.page?.session;
  if (oldMachines.has(o.machine_id) || !s || s.external || isStarting(s.id) || s.status === "ended") return null;

  try {
    const modes = await invoke<AgentModes | null>("agent_modes", where(o), AGENT_READ_WAIT_MS);
    if (!modes) oldMachines.add(o.machine_id);
    return modes;
  } catch (e) {
    console.warn("agent modes not read", e);
    return openModes();
  }
}

/// Whether the open session offers modes to switch between.
export function hasModes() {
  const s = AG.page?.session;
  return !!s && !s.external && s.status !== "ended" && (openModes()?.modes.length ?? 0) > 1;
}

/// Switches the open session to the mode after the one it is in; false when
/// it offers none.
export function cycleMode() {
  const m = openModes();
  if (!m || !hasModes()) return false;
  if (switching) return true;
  const next = m.modes[(m.modes.findIndex(x => x.id === m.current) + 1) % m.modes.length];
  m.current = next.id;
  switching = true;
  drawModes();
  // A refusal says why, and the next read shows the mode it stayed in.
  steerAgent({ mode: { mode: next.id } }).finally(() => { switching = false; });
  return true;
}

/// The agents' full access modes, the one Beep runs them in: no pill for it.
const FULL_MODES = new Set(["bypassPermissions", "agent-full-access"]);

export function drawModes() {
  const m = openModes(), s = AG.page?.session, q = s?.question;
  const name = m?.modes.find(x => x.id === m.current)?.name ?? m?.current ?? "";
  showAgentControl("mode", hasModes() && !FULL_MODES.has(m?.current ?? "") ? <AgentMode name={name} plan={m?.current === "plan"} /> : null);
  showAgentControl("plan", s && q && m?.plan != null ? <AgentPlan harness={agentKind(s.agent).name} plan={m.plan} question={q} approvalKey={questionKey(q, openOf(s))} answering={AG.answering} /> : null);
}
