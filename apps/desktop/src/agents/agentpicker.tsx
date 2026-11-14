import { createRef, type ReactElement, type RefObject } from "react";
import { AgentPicker, AgentPillContent } from "./agentpicker-view.tsx";
// The harness, model and thinking picker, as the start form and a project's
// agent bar both show it: the pill that opens it, the popover with its rail,
// model column and thinking column, and where it goes on screen.

import { type AgentModels, MODELS_KEPT_MS } from "./agentmodels.ts";
import { keepEffort, modelLabel, modelSearch, pillDetail, searchedHarnesses, searchRows, type PickRow } from "./agentpick.ts";
import { invoke, logError } from "../ui/tauri.ts";

// ----- Models -----

/// The models each machine's agents offer, by `machine/agent`: `null` when it
/// could not say, `undefined` in `list` until the first answer.
const MODELS = new Map<string, ModelsEntry>();
interface ModelsEntry { list: AgentModels | null | undefined; at: number; status: "idle" | "loading" | "ready" | "failed"; waiting: (() => void)[] }
const MODELS_RETRY_MS = 10_000;

/// Where the lists are kept between launches; both windows share it.
const MODELS_STORE = "beep.agentModels.v1";
/// Lists older than this are not shown again after a restart.
const MODELS_STORE_MAX_MS = 14 * 24 * 60 * 60 * 1000;

function restoreModels() {
  let kept: Record<string, { list: AgentModels; at: number }> = {};
  try { kept = JSON.parse(localStorage.getItem(MODELS_STORE) || "{}") || {}; } catch { return; }
  const now = Date.now();

  for (const [key, v] of Object.entries(kept)) {
    if (v && v.list && typeof v.at === "number" && now - v.at < MODELS_STORE_MAX_MS) {
      // Shown at once, and asked again the first time it is wanted.
      MODELS.set(key, { list: v.list, at: 0, status: "idle", waiting: [] });
    }
  }
}

function storeModels() {
  const out: Record<string, { list: AgentModels; at: number }> = {};
  for (const [key, v] of MODELS) if (v.list) out[key] = { list: v.list, at: v.at || Date.now() };
  try { localStorage.setItem(MODELS_STORE, JSON.stringify(out)); } catch { /* kept for this launch only */ }
}

restoreModels();

/// Asks `machine` for `agent`'s models unless a request is out; `loaded`
/// runs once the answer comes, including a failed refresh.
function askModels(key: string, machine: string, agent: string, loaded: (() => void) | null) {
  let entry = MODELS.get(key);
  if (!entry) MODELS.set(key, entry = { list: undefined, at: 0, status: "idle", waiting: [] });
  if (loaded) entry.waiting.push(loaded);
  if (entry.status === "loading") return;
  entry.status = "loading";
  invoke<AgentModels | null>("agent_models", { machineId: machine, agent })
    .catch((error: unknown) => {
      logError(`Model lookup failed: machine=${machine} harness=${agent}: ${String(error)}`);
      return null;
    })
    .then(list => {
      const e = MODELS.get(key)!;
      const waiting = e.waiting;
      // A failed refresh keeps the list shown.
      const next = list ?? (e.list === undefined ? null : e.list);
      MODELS.set(key, { list: next, at: Date.now(), status: list ? "ready" : "failed", waiting: [] });
      if (list) storeModels();
      for (const tell of waiting) tell();
    });
}

/// Whether `e` holds an answer read lately.
const fresh = (e: ModelsEntry | undefined) => !!e && (e.status === "ready" || e.status === "failed")
  && Date.now() - e.at <= (e.status === "failed" ? MODELS_RETRY_MS : MODELS_KEPT_MS);

/// The models `agent` offers on `machine` as known now: `undefined` only before
/// the first answer.
export function modelsOn(machine: string, agent: string, loaded: () => void): AgentModels | null | undefined {
  const key = `${machine}/${agent}`, known = MODELS.get(key);
  if (fresh(known)) return known!.list;
  askModels(key, machine, agent, loaded);
  return known?.list;
}

/// Reads the models of every harness on `hosts` ahead of the picker, such
/// as when a machine connects; lists read lately are left alone.
export function preloadModels(hosts: readonly { machine_id: string; agents: readonly string[] }[]) {
  for (const h of hosts) {
    for (const agent of h.agents) {
      const key = `${h.machine_id}/${agent}`;
      if (!fresh(MODELS.get(key))) askModels(key, h.machine_id, agent, null);
    }
  }
}

/// The models known for `agent` on `machine`, without asking.
export const knownModels = (machine: string | null, agent: string | null) => machine && agent ? MODELS.get(`${machine}/${agent}`)?.list : undefined;

// ----- Pill and popover -----

/// What is picked. The caller owns it; the picker changes it.
export interface Picked {
  agent: string;
  /// Empty for the harness's own.
  model: string;
  effort: string | null;
  query: string;
}

export interface PickerOpts {
  render: (view: ReactElement | null) => void;
  picked: Picked;
  /// The harnesses the machine has, and those any machine has.
  offered: readonly string[];
  everywhere: readonly string[];
  /// The machine models are listed on; null when there is none yet.
  machine: { id: string; name: string } | null;
  /// What the model column says without a machine.
  noMachine: string;
  /// The pill the picker opens from, and the area it stays inside.
  anchor: () => HTMLElement | null;
  bounds: () => HTMLElement | null;
  /// After a pick, or once the models are read ("models"); the caller
  /// redraws. A model pick is meant to close the picker.
  changed: (what: "agent" | "model" | "effort" | "models") => void;
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
  const search = modelSearch(p.query, o.offered);
  const machine = o.machine;
  const harnesses = machine ? searchedHarnesses(search, p.agent, o.offered) : [p.agent];
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
  const data = machine ? searchRows(lists, search) : { rows: [{ value: "", label: "Default", note: "its own", agent: p.agent }], note: o.noMachine };
  const note = failed ? "Couldn't load models." : loading ? "Asking the machine which models it has…" : data.note;
  const onRetry = () => {
    const current = key ? MODELS.get(key) : undefined;
    if (current && current.status !== "loading") current.status = "idle";
    drawPicker(el, o);
  };
  o.render(
    <AgentPicker
      picked={p} effort={keepEffort(p.agent, search.effort)} offered={o.offered} everywhere={o.everywhere} machineName={o.machine?.name ?? null} machineId={o.machine?.id ?? null}
      rows={data.rows} note={note} loading={loading} failed={failed}
      onRetry={onRetry}
      onQuery={(query: string) => { p.query = query; drawPicker(el, o); }}
      onAgent={(next: string) => { p.agent = next; p.query = ""; p.model = ""; o.changed("agent"); }}
      onModel={(row: PickRow) => {
        const effort = keepEffort(row.agent, search.effort);
        if (row.agent !== p.agent) p.effort = effort ?? keepEffort(row.agent, p.effort);
        else if (effort) p.effort = effort;
        p.agent = row.agent; p.model = row.value; p.query = "";
        o.changed("model");
      }}
      onEffort={(next: string | null) => { p.effort = next; o.changed("effort"); }}
      anchor={o.anchor} bounds={o.bounds} search={field}
    />,
  );
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
