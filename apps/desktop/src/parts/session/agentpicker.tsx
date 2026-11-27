import { createRef, type ReactElement, type RefObject } from "react";
import { AgentPicker, AgentPillContent, StepPicker } from "./agentpicker-view.tsx";
// The harness, model and thinking picker, as the start form and a project's
// agent bar both show it: the pill that opens it, the popover with its rail,
// model column and thinking column, and where it goes on screen.

import { MODELS, knownModels, modelsOn } from "../../stores/agents/model-store.ts";
import { type Allowed, allowedOf, keepEffort, modelLabel, modelSearch, pillDetail, searchedHarnesses, searchRows, type MachineRow, type PickRow } from "../../stores/agents/agentpick.ts";

// ----- Pill and popover -----

/// What is picked. The caller owns it; the picker changes it.
export interface Picked {
  agent: string;
  /// Empty for the harness's own.
  model: string;
  effort: string | null;
  fast: boolean | null;
  query: string;
}

export interface PickerOpts {
  render: (view: ReactElement | null) => void;
  picked: Picked;
  /// The harnesses the machine has, and those any machine has.
  offered: readonly string[];
  everywhere: readonly string[];
  /// The harnesses the project allows; null for every one.
  allowed: Allowed;
  /// The machine models are listed on; null when there is none yet.
  machine: { id: string; name: string } | null;
  /// What the model column says without a machine.
  noMachine: string;
  /// The picker under an agent button asks in steps, with the Machine step's
  /// machines and picking one (`machines` null where the machine is fixed);
  /// null for the plain picker.
  steps: { machines: { rows: readonly MachineRow[]; pick: (id: string) => void } | null } | null;
  /// The pill the picker opens from.
  anchor: () => HTMLElement | null;
  /// After a pick, or once the models are read ("models"); the caller
  /// redraws. In the plain picker a model pick is meant to close it; in
  /// steps, "done" follows the last step.
  changed: (what: "agent" | "model" | "effort" | "fast" | "models" | "done") => void;
}

/// The pill's inside: the harness badge, thinking level and model.
export function pillContent(agent: string, model: string, effort: string | null, machine: string | null) {
  const detail = pillDetail(modelLabel(knownModels(machine, agent), model), agent, effort);
  return <AgentPillContent agent={agent} detail={detail} />;
}

// The host's owner renders the picker; it must also clear it on close so
// search state and async model callbacks cannot outlive that host.
const renderers = new WeakMap<HTMLElement, (view: ReactElement | null) => void>();
/// The search field of the picker drawn in each host.
const searches = new WeakMap<HTMLElement, RefObject<HTMLInputElement | null>>();
/// The options each open picker was last drawn with, for redrawing it when its models arrive.
const drawn = new WeakMap<HTMLElement, PickerOpts>();

/// Draws the picker into `el` and places it by its pill.
export function drawPicker(el: HTMLElement, o: PickerOpts) {
  drawn.set(el, o);
  const p = o.picked;
  renderers.set(el, o.render);
  let field = searches.get(el);
  if (!field) searches.set(el, field = createRef<HTMLInputElement>());
  // In steps the harness is picked before the models: the search stays on it.
  const offered = allowedOf(o.offered, o.allowed), everywhere = allowedOf(o.everywhere, o.allowed);
  const search = modelSearch(p.query, o.steps ? [p.agent] : offered);
  const machine = o.machine;
  const harnesses = machine && !o.steps ? searchedHarnesses(search, p.agent, offered) : [p.agent];
  const lists = harnesses.map(agent => ({ agent, list: machine ? modelsOn(machine.id, agent, () => {
    const current = drawn.get(el);
    if (!current || !el.isConnected) return;
    drawPicker(el, current);
    current.changed("models");
  }) : null }));
  // Loading and failure are told for one harness; across several, those
  // still asked join as they answer.
  const key = machine && harnesses.length === 1 ? `${machine.id}/${harnesses[0]}` : null;
  const entry = key ? MODELS.get(key) : undefined;
  const failed = entry?.status === "failed";
  const loading = entry?.status === "loading" && !lists[0]?.list;
  const data = machine ? searchRows(lists, search) : { rows: [{ value: "", label: "Default", note: "its own", agent: p.agent, effort: null, fast: null }], note: o.noMachine };
  const note = failed ? "Couldn't load models." : loading ? "Asking the machine which models it has…" : data.note;
  const onRetry = () => {
    const current = key ? MODELS.get(key) : undefined;
    if (current && current.status !== "loading") current.status = "idle";
    drawPicker(el, o);
  };
  const own = lists.find(list => list.agent === p.agent)?.list ?? null;
  const props = {
    model: own?.models.find(model => model.id === (p.model || own.current)) ?? null,
    picked: p, offered, everywhere, allowed: o.allowed, machineName: o.machine?.name ?? null, machineId: o.machine?.id ?? null,
    rows: data.rows, note, loading, failed, onRetry,
    onQuery: (query: string) => { p.query = query; drawPicker(el, o); },
    onAgent: (next: string) => { p.agent = next; p.query = ""; p.model = ""; o.changed("agent"); },
    onModel: (row: PickRow) => {
      const effort = keepEffort(row.agent, row.effort);
      if (row.agent !== p.agent) p.effort = effort ?? keepEffort(row.agent, p.effort);
      else if (effort) p.effort = effort;
      p.agent = row.agent; p.model = row.value; p.query = "";
      o.changed("model");
    },
    onFast: (fast: boolean) => { p.fast = fast; o.changed("fast"); },
    onEffort: (next: string | null) => { p.effort = next; o.changed("effort"); },
    anchor: o.anchor, search: field,
  };
  const steps = o.steps;
  o.render(steps
    ? <StepPicker {...props} modelName={modelLabel(own, p.model)} machines={steps.machines?.rows ?? null}
      onMachine={(id: string) => steps.machines?.pick(id)} onDone={() => o.changed("done")} />
    : <AgentPicker {...props} />);
}

/// Focuses the picker's search, when it shows in `el`.
export function focusSearch(el: HTMLElement) {
  searches.get(el)?.current?.focus();
}

/// Unmounts the picker and releases its React root.
export function closedPicker(el: HTMLElement) {
  drawn.delete(el);
  const render = renderers.get(el);
  if (render) { renderers.delete(el); render(null); }
}
