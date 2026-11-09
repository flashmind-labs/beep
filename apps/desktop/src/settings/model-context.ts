// Harness-reported context limits and shared model compaction overrides.
import type { invoke as Invoke } from "../ui/tauri.ts";
import type { AgentModels } from "../agents/agentmodels.ts";
import { modelDraft, modelProblem, modelRules } from "./model-context-state.ts";
import type { ModelCompact, ModelCompactDraft } from "./model-context-state.ts";

export interface ContextState { models: AgentModels | null; saved: ModelCompact[]; draft: ModelCompactDraft[]; loading: boolean; saving: boolean; error: string; loaded: boolean; filter: string; defaultModel: string; savedDefault: string; defaultText: string; customIndexes: number[]; unfilteredIndexes: number[]; savingDefault: boolean; defaultError: string }
export const supportsOverrides = (agent: string) => ["codex", "claude", "pi"].includes(agent);
const initial = (): ContextState => ({ models: null, saved: [], draft: [], loading: false, saving: false, error: "", loaded: false, filter: "", defaultModel: "", savedDefault: "", defaultText: "", customIndexes: [], unfilteredIndexes: [], savingDefault: false, defaultError: "" });

export function modelContextPanel(invoke: typeof Invoke) {
  const states = new Map<string, ContextState>();
  const listeners = new Set<() => void>();
  const revisions = new Map<string, number>();
  const key = (machine: string, agent: string) => `${machine}\0${agent}`;
  function snapshot(machine: string, agent: string) {
    const id = key(machine, agent);
    let state = states.get(id);
    if (!state) { state = initial(); states.set(id, state); }
    return state;
  }
  function update(machine: string, agent: string, change: Partial<ContextState>) {
    states.set(key(machine, agent), { ...snapshot(machine, agent), ...change });
    for (const listener of listeners) listener();
  }
  function invalidate(machine: string, agent: string) {
    for (const id of states.keys()) if (id !== key(machine, agent) && id.endsWith(`\0${agent}`)) {
      revisions.set(id, (revisions.get(id) ?? 0) + 1);
      states.delete(id);
    }
    for (const listener of listeners) listener();
  }
  async function load(machine: string, agent: string) {
    if (snapshot(machine, agent).loading) return;
    update(machine, agent, { loading: true, error: "" });
    const id = key(machine, agent), revision = revisions.get(id) ?? 0;
    const current = () => (revisions.get(id) ?? 0) === revision;
    try {
      const [models, rules, defaultModel] = await Promise.all([
        invoke<AgentModels | null>("agent_models", { machineId: machine, agent }),
        supportsOverrides(agent) ? invoke<ModelCompact[]>("harness_model_compacts", { agent }) : Promise.resolve([]),
        invoke<string | null>("harness_default_model", { agent }),
      ]);
      if (current()) update(machine, agent, { models, saved: rules, draft: modelDraft(rules), customIndexes: rules.flatMap((rule, i) => models?.models.some(model => model.id === rule.model) ? [] : [i]), unfilteredIndexes: [], defaultModel: defaultModel ?? "", defaultText: models?.models.find(model => model.id === defaultModel)?.name ?? defaultModel ?? "", savedDefault: defaultModel ?? "", loaded: true });
    } catch (error) { if (current()) update(machine, agent, { error: String(error) }); }
    finally { if (current()) update(machine, agent, { loading: false }); }
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
  return { snapshot, update, load, save, saveDefault, subscribe(listener: () => void) { listeners.add(listener); return () => listeners.delete(listener); } };
}
export type ModelContextPanel = ReturnType<typeof modelContextPanel>;
