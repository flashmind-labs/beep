// The session's mode: the pill by its field, Shift+Tab, and the plan waiting
// for approval above the field.
import { $ } from "../bar/core.ts";
import { answerHtml } from "./turnrows.ts";
import { type AgentModes, type OpenAgent, AG, agentKind, where } from "./agents.ts";
import { steerAgent } from "./messages.ts";
import { esc } from "../ui/format.ts";
import { showHtml } from "../ui/patch.ts";
import { invoke } from "../ui/tauri.ts";

/// The server did not know the request, so it is not asked again.
let oldServer = false;
/// A switch on its way; a press meanwhile is dropped.
let switching = false;

/// The modes of the open session; null when it has none to switch between.
export async function readModes(o: OpenAgent) {
  const s = AG.page?.session;
  if (oldServer || !s || s.external || s.status === "ended") return null;
  try {
    const modes = await invoke<AgentModes | null>("agent_modes", where(o));
    if (!modes) oldServer = true;
    return modes;
  } catch (e) {
    console.warn("agent modes not read", e);
    return AG.modes;
  }
}

/// Whether the open session offers modes to switch between.
export function hasModes() {
  const s = AG.page?.session;
  return !!s && !s.external && s.status !== "ended" && (AG.modes?.modes.length ?? 0) > 1;
}

/// Switches the open session to the mode after the one it is in; false when
/// it offers none.
export function cycleMode() {
  const m = AG.modes;
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

export function drawModes() {
  const m = AG.modes, s = AG.page?.session, q = s?.question;
  const name = m?.modes.find(x => x.id === m.current)?.name ?? m?.current ?? "";
  showHtml($("#agmode"), hasModes() ? `<button class="agc-pill agmode${m?.current === "plan" ? " on" : ""}" id="agmodeb" title="Switch mode (Shift+Tab)">${esc(name)}</button>` : "");
  // The node puts a plan to the user as a question with these two answers.
  const plan = s && q && m?.plan != null ? `<div class="ag-plan"><div class="h"><b>Plan ready</b><small>${esc(agentKind(s.agent).name)} waits for you</small></div>
    ${m.plan ? `<div class="dcoderen agent md">${answerHtml(m.plan)}</div>` : ""}
    <div class="acts">${q.options.map((o, i) => `<button class="dbtn${o.allow ? " pri" : ""}" data-agopt="${i}"${q.id === AG.answering ? " disabled" : ""}>${esc(o.name)}</button>`).join("")}</div></div>` : "";
  showHtml($("#agplan"), plan);
}
