// The model and thinking pill by a session's field, and switching them.
import { createElement } from "react";
import { showAgentControl } from "./composer-ui.tsx";
import { modelId } from "./agentpick.ts";
import { closedPicker, knownModels, modelsOn, drawPicker, type Picked, pillInner } from "./agentpicker.ts";
import { $, bar } from "../bar/core.ts";
import { type AgentRow, AG, agentKind } from "./agents.ts";
import { SWITCHES, sessionPicks } from "./session.ts";
import { drawAgentBody } from "./logview.ts";
import { steerAgent } from "./messages.ts";

/// The pill by the field: the harness, the model and thinking switched to.
/// A harness that cannot switch in a running session shows it disabled.
export function drawModelPill() {
  const el = $("#agmodel"), field = $("#agfield"), s = AG.page?.session;
  if (!el || !field || !s) return;
  const can = SWITCHES.has(s.agent) && !s.external && s.status !== "ended";
  const p = sessionPicks.get(s.id) ?? { model: "", effort: null };
  const model = AG.page?.model || p.model;
  if (model) modelsOn(s.machine_id, s.agent, () => { if (AG.page?.session.id === s.id) drawModelPill(); });
  const why = can ? "Switch the model or thinking; applies from the next turn" : `${agentKind(s.agent).name} cannot switch models in a running session`;
  showAgentControl(field, el, createElement("button", { className: "agc-pill agm", id: "agmodelb", "aria-expanded": AG.modelOpen, title: why, disabled: !can, dangerouslySetInnerHTML: { __html: pillInner(s.agent, model || "Model unknown", p.effort, s.machine_id) } }));
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
  $("#agmsg")?.focus();
  return true;
}

/// The picker by the pill, for the session's own harness: a model or a
/// thinking level picked goes to the session for its next turn.
function drawModelPicker() {
  const el = $("#agpickm"), s = AG.page?.session;
  if (!el) return;
  if (!AG.modelOpen || !s) { closedPicker(el); return; }
  const now = sessionPicks.get(s.id) ?? { model: "", effort: null };
  const picked: Picked = { agent: s.agent, model: AG.page?.model || now.model, effort: now.effort, query: "" };
  drawPicker(el, {
    render: view => { const field = $("#agfield"); if (field?.isConnected && el.isConnected) showAgentControl(field, el, view); },
    picked,
    offered: [s.agent],
    everywhere: [s.agent],
    machine: { id: s.machine_id, name: s.machine_name },
    noMachine: "",
    anchor: () => $("#agmodelb"),
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
