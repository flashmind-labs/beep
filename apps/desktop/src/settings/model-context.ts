// Harness-reported context limits and shared model compaction overrides.
import { createStore } from "zustand";
import { latest, STALE } from "../ui/active.ts";
import type { invoke as Invoke } from "../ui/tauri.ts";
import type { AgentModels } from "../agents/agentmodels.ts";
import { modelDraft, modelProblem, modelRules } from "./model-context-state.ts";
import type { ModelCompact, ModelCompactDraft } from "./model-context-state.ts";

export interface ContextState { models: AgentModels | null; saved: ModelCompact[]; draft: ModelCompactDraft[]; loading: boolean; saving: boolean; error: string; loaded: boolean; filter: string; defaultModel: string; savedDefault: string; defaultText: string; customIndexes: number[]; unfilteredIndexes: number[]; savingDefault: boolean; defaultError: string }
export const supportsOverrides = (agent: string) => ["codex", "claude", "pi"].includes(agent);
const INITIAL: ContextState = { models: null, saved: [], draft: [], loading: false, saving: false, error: "", loaded: false, filter: "", defaultModel: "", savedDefault: "", defaultText: "", customIndexes: [], unfilteredIndexes: [], savingDefault: false, defaultError: "" };

/// Each machine's and harness's panel state, keyed by `contextKey`.
type ContextStates = Readonly<Record<string, ContextState>>;
const contextKey = (machine: string, agent: string) => `${machine}\0${agent}`;

/// The panel state of `agent` on `machine` in `states`.
export const contextOf = (states: ContextStates, machine: string, agent: string) => states[contextKey(machine, agent)] ?? INITIAL;

export function modelContextPanel(invoke: typeof Invoke, readModels: (machine: string, agent: string) => Promise<AgentModels | null>) {
  const states = createStore<ContextStates>()(() => ({}));
  // A save elsewhere makes each other machine's read of the same harness stale.
  const reads = new Map<string, ReturnType<typeof latest>>();
  const snapshot = (machine: string, agent: string) => contextOf(states.getState(), machine, agent);

  function update(machine: string, agent: string, change: Partial<ContextState>) {
    const id = contextKey(machine, agent);
    states.setState(all => ({ ...all, [id]: { ...(all[id] ?? INITIAL), ...change } }));
  }

  function invalidate(machine: string, agent: string) {
    const keep = contextKey(machine, agent);
    states.setState(all => Object.fromEntries(Object.entries(all).filter(([id]) => {
      if (id === keep || !id.endsWith(`\0${agent}`)) return true;
      reads.get(id)?.cancel();
      return false;
    })), true);
  }

  async function load(machine: string, agent: string) {
    if (snapshot(machine, agent).loading) return;
    update(machine, agent, { loading: true, error: "" });
    const id = contextKey(machine, agent);
    const read = reads.get(id) ?? latest();
    reads.set(id, read);

    try {
      const answer = await read.run(Promise.all([
        readModels(machine, agent),
        supportsOverrides(agent) ? invoke<ModelCompact[]>("harness_model_compacts", { agent }) : Promise.resolve([]),
        invoke<string | null>("harness_default_model", { agent }),
      ]));
      if (answer === STALE) return;

      const [models, rules, defaultModel] = answer;
      update(machine, agent, { models, saved: rules, draft: modelDraft(rules), customIndexes: rules.flatMap((rule, i) => models?.models.some(model => model.id === rule.model) ? [] : [i]), unfilteredIndexes: [], defaultModel: defaultModel ?? "", defaultText: models?.models.find(model => model.id === defaultModel)?.name ?? defaultModel ?? "", savedDefault: defaultModel ?? "", loaded: true, loading: false });
    } catch (error) { update(machine, agent, { error: String(error), loading: false }); }
  }

  async function save(machine: string, agent: string) {
    const state = snapshot(machine, agent);
    if (state.saving || modelProblem(state.draft)) return;
    const rules = modelRules(state.draft);
    update(machine, agent, { saving: true, error: "" });
    try {
      await invoke("harness_model_compacts_save", { agent, rules });
      update(machine, agent, { saved: rules, draft: modelDraft(rules), customIndexes: rules.flatMap((rule, i) => state.models?.models.some(model => model.id === rule.model) ? [] : [i]), unfilteredIndexes: [] });
      invalidate(machine, agent);
    } catch (error) { update(machine, agent, { error: String(error) }); }
    finally { update(machine, agent, { saving: false }); }
  }
  async function saveDefault(machine: string, agent: string) {
    const state = snapshot(machine, agent), model = state.defaultModel.trim();
    if (state.savingDefault || model === state.savedDefault) return;
    update(machine, agent, { savingDefault: true, defaultError: "" });
    try {
      await invoke("harness_default_model_save", { agent, model: model || null });
      update(machine, agent, { savedDefault: model, defaultModel: model, defaultText: state.models?.models.find(item => item.id === model)?.name ?? model });
      invalidate(machine, agent);
    } catch (error) { update(machine, agent, { defaultError: String(error) }); }
    finally { update(machine, agent, { savingDefault: false }); }
  }
  return { states, snapshot, update, load, save, saveDefault };
}
export type ModelContextPanel = ReturnType<typeof modelContextPanel>;
