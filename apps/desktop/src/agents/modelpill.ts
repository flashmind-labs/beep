// The model and thinking pill by a session's field, and switching them.
import { agentMessage } from "./session-frame.tsx";
import { createElement, createRef } from "react";
import { modelPickHost, showAgentControl, showAgentControlFor } from "./composer-ui.tsx";
import type { SessionPick } from "./agentmodels.ts";
import { isNativeSession } from "./native-session-state.ts";
import { modelId } from "./agentpick.ts";
import { closedPicker, knownModels, modelsOn, drawPicker, type Picked, pillContent } from "./agentpicker.ts";
import { bar } from "../bar/core.ts";
import { type AgentRow, AG, agentKind, canTakeOver } from "./agents.ts";
import { SWITCHES, sessionPicks } from "./session.ts";
import { drawAgentBody } from "./logview.ts";
import { steerAgent } from "./messages.ts";

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
  const p = sessionPicks.get(s.id) ?? { model: "", effort: null };
  const model = resumePicks.get(s.id)?.model ?? (AG.page?.model || p.model);
  const models = modelsOn(s.machine_id, s.agent, () => { if (AG.page?.session.id === s.id) drawModelPill(); });
  const displayModel = model || (models?.current ? "" : "Model unknown");
  const why = can ? "Switch the model or thinking; applies from the next turn" : `${agentKind(s.agent).name} cannot switch models in a running session`;
  const content = pillContent(s.agent, displayModel, p.effort, s.machine_id);
  const signature = JSON.stringify([s.agent, content.props.detail, AG.modelOpen, why, can]);
  showAgentControlFor("model", signature, () => createElement("button", { ref: modelPill, className: "agc-pill agm", id: "agmodelb", "aria-expanded": AG.modelOpen, title: why, disabled: !can }, content));
}

export function toggleModelPicker() {
  AG.modelOpen = !AG.modelOpen;
  drawModelPicker();
  drawModelPill();
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

/// The picker by the pill, for the session's own harness: a model or a
/// thinking level picked goes to the session for its next turn.
function drawModelPicker() {
  const el = modelPickHost.current, s = AG.page?.session;
  if (!el) return;
  if (!AG.modelOpen || !s) { closedPicker(el); return; }
  const now = sessionPicks.get(s.id) ?? { model: "", effort: null };
  const pending = resumePicks.get(s.id);
  const picked: Picked = { agent: s.agent, model: pending?.model ?? (AG.page?.model || now.model), effort: pending ? pending.effort : now.effort, query: "" };
  drawPicker(el, {
    render: view => { if (el.isConnected) showAgentControl("pick", view); },
    picked,
    offered: [s.agent],
    everywhere: [s.agent],
    machine: { id: s.machine_id, name: s.machine_name },
    noMachine: "",
    anchor: () => modelPill.current,
    bounds: () => bar,
    changed: what => {
      if (what === "models") return drawModelPill();
      if (what === "agent") return;
      if (what === "model") { AG.modelOpen = false; drawModelPicker(); }
      switchSessionModel(s, { model: picked.model, effort: picked.effort }, what);
    },
  });
}

/// Switches session `s` to the model or thinking in `next`.
export async function switchSessionModel(s: AgentRow, next: { model: string; effort: string | null }, what: "model" | "effort") {
  next = { ...next, model: modelId(knownModels(s.machine_id, s.agent), next.model) };
  if (s.status === "ended") {
    resumePicks.set(s.id, next);
    sessionPicks.set(s.id, next);
    drawModelPicker();
    drawModelPill();
    return;
  }

  const was = sessionPicks.get(s.id) ?? { model: "", effort: null };
  const model = what === "model" ? next.model || null : null, effort = what === "effort" ? next.effort : null;

  if (model || effort) {
    if (await steerAgent({ configure: { model, effort } })) sessionPicks.set(s.id, next);
  } else if (what === "model" ? was.model : was.effort) {
    AG.note = `A running session cannot go back to Default; pick ${what === "model" ? "a model" : "a level"} by name.`;
    drawAgentBody();
  }

  drawModelPicker();
  drawModelPill();
}
