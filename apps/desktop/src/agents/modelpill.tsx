// The model and thinking pill by a session's field, and switching them.
import { agentMessage } from "./session-frame.tsx";
import { createRef } from "react";
import { modelPickHost, showAgentControl, showAgentControlFor } from "./composer-ui.tsx";
import type { SessionPick } from "./agentmodels.ts";
import { isNativeSession } from "./native-session-state.ts";
import { modelId } from "./agentpick.ts";
import { closedPicker, knownModels, modelsOn, drawPicker, type Picked, pillContent } from "./agentpicker.tsx";
import { type AgentRow, AG, agentKind, canTakeOver } from "./agents.ts";
import { SWITCHES, sessionPicks } from "./session.tsx";
import { drawAgentBody } from "./logview.tsx";
import { steerAgent } from "./messages.ts";
import { perfLayer, perfOpen } from "../ui/perf.ts";

export const resumePicks = new Map<string, SessionPick>();
/// The model pill, which the picker opens from.
const modelPill = createRef<HTMLButtonElement>();

/// The pill by the field: the harness, the model and thinking switched to.
/// A harness that cannot switch in a running session shows it disabled.
export function drawModelPill() {
  const s = AG.page?.session;
  if (!s) return;
  const can = SWITCHES.has(s.agent) && !isNativeSession(s.id)
    && (s.status === "ended" ? !s.external || canTakeOver(s.agent) : !s.external);
  const p = sessionPicks.get(s.id) ?? { model: "", effort: null, fast: null };
  const model = resumePicks.get(s.id)?.model ?? (AG.page?.model || p.model);
  const models = modelsOn(s.machine_id, s.agent, () => { if (AG.page?.session.id === s.id) drawModelPill(); });
  const displayModel = model || (models?.current ? "" : "Model unknown");
  const why = can ? "Switch the model or thinking; applies from the next turn" : `${agentKind(s.agent).name} cannot switch models in a running session`;
  const content = pillContent(s.agent, displayModel, p.effort, s.machine_id);
  const signature = JSON.stringify([s.agent, content.props.detail, AG.modelOpen, why, can]);
  showAgentControlFor("model", signature, () => <button ref={modelPill} className="agc-pill agm" id="agmodelb" aria-expanded={AG.modelOpen} title={why} disabled={!can}>{content}</button>);
}

export function toggleModelPicker() {
  if (closeModelPicker()) return;
  if (modelPill.current?.disabled) return;
  AG.modelOpen = true;
  const opened = perfOpen("picker");
  drawModelPicker();
  drawModelPill();
  opened();
}

/// Closes the model picker; true when it was open, so Esc goes no further.
export function closeModelPicker() {
  if (!AG.modelOpen) return false;
  AG.modelOpen = false;
  drawModelPicker();
  drawModelPill();
  agentMessage.current?.focus();
  return true;
}

/// Interactions while the picker shows are the picker's.
let pickerShown = false;
function pickerLayer(on: boolean) {
  if (on === pickerShown) return;
  pickerShown = on;
  perfLayer("picker", on);
}

/// The picker by the pill, for the session's own harness: a model or a
/// thinking level picked goes to the session for its next turn.
function drawModelPicker() {
  const el = modelPickHost.current, s = AG.page?.session;
  if (!el) return;
  pickerLayer(AG.modelOpen && !!s);
  if (!AG.modelOpen || !s) { closedPicker(el); return; }
  const now = sessionPicks.get(s.id) ?? { model: "", effort: null, fast: null };
  const pending = resumePicks.get(s.id);
  const was = pending ? pending.effort : now.effort;
  const picked: Picked = { agent: s.agent, model: pending?.model ?? (AG.page?.model || now.model), effort: was, fast: pending ? pending.fast : now.fast, query: "" };
  drawPicker(el, {
    render: view => { if (el.isConnected) showAgentControl("pick", view); },
    picked,
    offered: [s.agent],
    everywhere: [s.agent],
    machine: { id: s.machine_id, name: s.machine_name },
    noMachine: "",
    steps: null,
    anchor: () => modelPill.current,
    changed: what => {
      if (what === "models") return drawModelPill();
      if (what === "agent" || what === "done") return;
      // A search such as `opus high` picks the thinking level with the model.
      const both = what === "model" && picked.effort !== was;
      if (what === "model") { AG.modelOpen = false; drawModelPicker(); }
      switchSessionModel(s, { model: picked.model, effort: picked.effort, fast: picked.fast }, both ? "both" : what);
    },
  });
}

/// Switches session `s` to the model, the thinking or both in `next`.
export async function switchSessionModel(s: AgentRow, next: SessionPick, what: "model" | "effort" | "fast" | "both") {
  next = { ...next, model: modelId(knownModels(s.machine_id, s.agent), next.model) };
  if (s.status === "ended") {
    resumePicks.set(s.id, next);
    sessionPicks.set(s.id, next);
    drawModelPicker();
    drawModelPill();
    return;
  }

  const was = sessionPicks.get(s.id) ?? { model: "", effort: null, fast: null };
  const model = what === "model" || what === "both" ? next.model || null : null, effort = what === "effort" || what === "both" ? next.effort : null;

  const fast = what === "fast" ? next.fast : null;
  if (model || effort || fast !== null) {
    if (await steerAgent({ configure: { model, effort, fast } })) sessionPicks.set(s.id, next);
  } else if (what === "effort" ? was.effort : was.model) {
    AG.note = `A running session cannot go back to Default; pick ${what === "effort" ? "a level" : "a model"} by name.`;
    drawAgentBody();
  }

  drawModelPicker();
  drawModelPill();
}
