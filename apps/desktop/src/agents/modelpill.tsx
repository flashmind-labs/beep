// The model and thinking pill by a session's field, and switching them.
import { agentMessage } from "./session-frame.tsx";
import { createRef } from "react";
import { modelPickHost, showAgentControl, showAgentControlFor } from "./composer-ui.tsx";
import type { SessionPick } from "./agentmodels.ts";
import { isNativeSession } from "./native-session-state.ts";
import { modelId } from "./agentpick.ts";
import { closedPicker, drawPicker, type Picked, pillContent } from "./agentpicker.tsx";
import { knownModels, modelsOn } from "./model-store.ts";
import { type AgentRow, AG, agentKind, canTakeOver } from "./agents.ts";
import { SWITCHES, sessionPicks, setNote } from "./session.tsx";
import { drawAgentBody } from "./logview.tsx";
import { resendPrompt, steerAgent } from "./messages.ts";
import { perfLayer, perfOpen } from "../ui/perf.ts";

export const resumePicks = new Map<string, SessionPick>();
/// The model pill, which the picker opens from.
const modelPill = createRef<HTMLButtonElement>();
/// The session whose failed prompt goes again once a model is picked.
let retryIn: string | null = null;

/// Whether session `s` can switch its model or thinking.
export const canSwitchModel = (s: AgentRow) => SWITCHES.has(s.agent) && !isNativeSession(s.id)
  && (s.status === "ended" ? !s.external || canTakeOver(s.agent) : !s.external);

/// The pill by the field: the harness, the model and thinking switched to.
/// A harness that cannot switch in a running session shows it disabled.
export function drawModelPill() {
  const s = AG.page?.session;
  if (!s) return;
  const can = canSwitchModel(s);
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

/// Opens the picker for a failed turn: the model picked runs its prompt again.
export function retryWithModel() {
  const s = AG.page?.session;
  if (!s) return;

  retryIn = s.id;
  if (!AG.modelOpen) toggleModelPicker();
}

/// Closes the model picker; true when it was open, so Esc goes no further.
export function closeModelPicker() {
  if (!AG.modelOpen) return false;
  retryIn = null;
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
    allowed: null,
    machine: { id: s.machine_id, name: s.machine_name },
    noMachine: "",
    steps: null,
    anchor: () => modelPill.current,
    changed: what => {
      if (what === "models") return drawModelPill();
      if (what === "agent" || what === "done") return;
      // A search such as `opus high` picks the thinking level with the model.
      const both = what === "model" && picked.effort !== was;
      const retry = what === "model" && retryIn === s.id;
      if (what === "model") { AG.modelOpen = false; retryIn = null; drawModelPicker(); }

      void switchSessionModel(s, { model: picked.model, effort: picked.effort, fast: picked.fast }, both ? "both" : what)
        .then(switched => { if (retry && switched) void resendPrompt(); });
    },
  });
}

/// Switches session `s` to the model, the thinking or both in `next`; true when it took.
export async function switchSessionModel(s: AgentRow, next: SessionPick, what: "model" | "effort" | "fast" | "both") {
  next = { ...next, model: modelId(knownModels(s.machine_id, s.agent), next.model) };
  if (s.status === "ended") {
    resumePicks.set(s.id, next);
    sessionPicks.set(s.id, next);
    drawModelPicker();
    drawModelPill();
    return true;
  }

  const hadWas = sessionPicks.has(s.id);
  const was = sessionPicks.get(s.id) ?? { model: "", effort: null, fast: null };
  const model = what === "model" || what === "both" ? next.model || null : null, effort = what === "effort" || what === "both" ? next.effort : null;

  const fast = what === "fast" ? next.fast : null;
  let switched = false;
  if (model || effort || fast !== null) {
    sessionPicks.set(s.id, next);
    drawModelPicker();
    drawModelPill();
    switched = await steerAgent({ configure: { model, effort, fast } });

    if (!switched) {
      if (hadWas) sessionPicks.set(s.id, was);
      else sessionPicks.delete(s.id);
    }
  } else if (what === "effort" ? was.effort : was.model) {
    setNote(`A running session cannot go back to Default; pick ${what === "effort" ? "a level" : "a model"} by name.`);
    drawAgentBody();
  }

  drawModelPicker();
  drawModelPill();
  return switched;
}
