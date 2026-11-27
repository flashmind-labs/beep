// The model and thinking pill by a session's field, and switching them.
import { agentMessage } from "../../stores/agents/session-frame-state.ts";
import { createRef } from "react";
import { modelPickHost, showAgentControl, showAgentControlFor } from "./composer-ui.tsx";
import type { SessionPick } from "../../stores/agents/agentmodels.ts";
import { isNativeSession } from "../../stores/agents/native-session-state.ts";
import { harnessesOn, keepEffort, modelId } from "../../stores/agents/agentpick.ts";
import { canContinueElsewhere } from "../../stores/agents/continue-state.ts";
import { closedPicker, drawPicker, type Picked, pillContent } from "./agentpicker.tsx";
import { knownModels, modelsOn } from "../../stores/agents/model-store.ts";
import { type AgentRow, AG, canTakeOver, ensureAgentHosts } from "../../stores/agents/agents-state.ts";
import { agentKind } from "../../stores/agents/agent-kind.ts";
import { SWITCHES, continuePicks, logStale, resumePicks, sessionPicks, setNote } from "./open-state.ts";
import { resendPrompt, steerAgent } from "./messages.ts";
import { perfLayer, perfOpen } from "../../ui/perf.ts";
import { sessionProject } from "../project/sessionproject.ts";
import { projectHarnesses, readProjectHarnesses } from "../project/project-harnesses.tsx";

/// The model pill, which the picker opens from.
const modelPill = createRef<HTMLButtonElement>();
/// The session whose failed prompt goes again once a model is picked.
let retryIn: string | null = null;

/// Whether session `s` can switch its model or thinking.
export const canSwitchModel = (s: AgentRow) => SWITCHES.has(s.agent) && !isNativeSession(s.id)
  && (s.status === "ended" ? !s.external || canTakeOver(s.agent) : !s.external);

/// The harnesses session `s`'s project allows, with its own; null for every one.
function allowedFor(s: AgentRow) {
  const allowed = projectHarnesses(sessionProject(s)?.name ?? null);
  return allowed && [...new Set([s.agent, ...allowed])];
}

/// The harnesses session `s` can go on with: every one its machine offers
/// and its project allows while it can be continued, else its own.
function harnessesFor(s: AgentRow) {
  return canContinueElsewhere(s) ? [...new Set([s.agent, ...harnessesOn(AG.hosts, s.machine_id, allowedFor(s))])] : [s.agent];
}

/// Whether the picker has another harness to offer for `s`.
export const canContinueWith = (s: AgentRow) => harnessesFor(s).length > 1;

/// The pill by the field: the harness, the model and thinking switched to.
/// A harness that cannot switch in a running session shows it disabled.
export function drawModelPill() {
  const s = AG.page?.session;
  if (!s) return;
  // A chat that began to work or ask goes on as it is.
  if (!canContinueElsewhere(s)) continuePicks.delete(s.id);
  const next = continuePicks.get(s.id);
  if (next) modelsOn(s.machine_id, next.agent, () => { if (AG.page?.session.id === s.id) drawModelPill(); });
  const can = canSwitchModel(s) || canContinueWith(s);
  const p = sessionPicks.get(s.id) ?? { model: "", effort: null, fast: null };
  const model = resumePicks.get(s.id)?.model ?? (AG.page?.model || p.model);
  const models = modelsOn(s.machine_id, s.agent, () => { if (AG.page?.session.id === s.id) drawModelPill(); });
  const displayModel = model || (models?.current ? "" : "Model unknown");
  const why = next ? `Your next message continues this chat with ${agentKind(next.agent).name}` : can ? "Switch the model or thinking; applies from the next turn" : `${agentKind(s.agent).name} cannot switch models in a running session`;
  const content = next ? pillContent(next.agent, next.model, next.effort, s.machine_id) : pillContent(s.agent, displayModel, p.effort, s.machine_id);
  const signature = JSON.stringify([next?.agent ?? s.agent, content.props.detail, AG.modelOpen, why, can]);
  showAgentControlFor("model", signature, () => <button ref={modelPill} className="agc-pill agm" id="agmodelb" aria-expanded={AG.modelOpen} title={why} disabled={!can}>{content}</button>);
}

export function toggleModelPicker() {
  if (closeModelPicker()) return;
  if (modelPill.current?.disabled) return;
  AG.modelOpen = true;
  // The machines' harnesses may not be read yet: the picker lists them once they are.
  if (!AG.hosts) void ensureAgentHosts().then(() => { if (AG.hosts && AG.modelOpen) { drawModelPicker(); drawModelPill(); } });
  // So are the harnesses its project allows.
  const project = AG.page ? sessionProject(AG.page.session)?.name : null;
  if (project) void readProjectHarnesses(project).then(() => { if (AG.modelOpen) { drawModelPicker(); drawModelPill(); } });
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

/// The picker by the pill: a model or a thinking level of the session's own
/// harness goes to the session for its next turn; one of another harness is
/// kept for its next message, which carries the chat on there.
function drawModelPicker() {
  const el = modelPickHost.current, s = AG.page?.session;
  if (!el) return;
  pickerLayer(AG.modelOpen && !!s);
  if (!AG.modelOpen || !s) { closedPicker(el); return; }
  const now = sessionPicks.get(s.id) ?? { model: "", effort: null, fast: null };
  const pending = resumePicks.get(s.id);
  const was = pending ? pending.effort : now.effort;
  const next = continuePicks.get(s.id);
  const picked: Picked = next ? { ...next, query: "" }
    : { agent: s.agent, model: pending?.model ?? (AG.page?.model || now.model), effort: was, fast: pending ? pending.fast : now.fast, query: "" };
  const offered = harnessesFor(s);
  drawPicker(el, {
    render: view => { if (el.isConnected) showAgentControl("pick", view); },
    picked,
    offered,
    everywhere: offered,
    allowed: allowedFor(s),
    machine: { id: s.machine_id, name: s.machine_name },
    noMachine: "",
    steps: null,
    anchor: () => modelPill.current,
    changed: what => {
      if (what === "models") return drawModelPill();
      if (what === "done") return;
      if (what === "agent") {
        // A harness other than its own is the chat's next one, on its default model.
        if (picked.agent === s.agent) continuePicks.delete(s.id);
        else continuePicks.set(s.id, { agent: picked.agent, model: "", effort: keepEffort(picked.agent, picked.effort), fast: null });
        drawModelPicker();
        drawModelPill();
        return;
      }

      if (picked.agent !== s.agent) {
        const model = modelId(knownModels(s.machine_id, picked.agent), picked.model);
        continuePicks.set(s.id, { agent: picked.agent, model, effort: picked.effort, fast: picked.fast });
        retryIn = null;
        if (what === "model") AG.modelOpen = false;
        drawModelPicker();
        drawModelPill();
        return;
      }

      // Back on its own harness: the chat stays here.
      continuePicks.delete(s.id);
      if (!canSwitchModel(s)) { AG.modelOpen = false; drawModelPicker(); drawModelPill(); return; }
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
    logStale.notify();
  }

  drawModelPicker();
  drawModelPill();
  return switched;
}
