// Harness-reported context limits and shared model compaction overrides.
import type { invoke as Invoke } from "../ui/tauri.ts";
import type { MakeButton, MakeEl } from "../ui/dom.ts";
import type { AgentModels } from "../agents/agentmodels.ts";
import { modelDraft, modelProblem, modelRules } from "./model-context-state.ts";
import type { ModelCompact, ModelCompactDraft } from "./model-context-state.ts";
interface Deps { invoke: typeof Invoke; h: MakeEl; button: MakeButton; redraw: () => void }
interface State { models: AgentModels | null; saved: ModelCompact[]; draft: ModelCompactDraft[]; loading: boolean; saving: boolean; error: string; loaded: boolean; filter: string; defaultModel: string; savedDefault: string; savingDefault: boolean; defaultError: string }
const supportsOverrides = (agent: string) => ["codex", "claude", "pi"].includes(agent);
export function modelContextPanel({ invoke, h, button, redraw }: Deps) {
  const states = new Map<string, State>();
  async function load(machine: string, agent: string, st: State) {
    st.loading = true;
    st.error = "";

    try {
      const [models, rules, defaultModel] = await Promise.all([
        invoke<AgentModels | null>("agent_models", { machineId: machine, agent }),
        supportsOverrides(agent) ? invoke<ModelCompact[]>("harness_model_compacts", { agent }) : Promise.resolve([]),
        invoke<string | null>("harness_default_model", { agent }),
      ]);
      st.models = models;
      st.defaultModel = defaultModel ?? "";
      st.savedDefault = st.defaultModel;
      st.saved = rules;
      st.draft = modelDraft(rules);
      st.loaded = true;
    } catch (e) { st.error = String(e); }
    finally { st.loading = false; redraw(); }
  }
  async function save(agent: string, st: State) {
    if (st.saving || modelProblem(st.draft)) return;
    st.saving = true;
    st.error = "";
    redraw();

    try {
      const rules = modelRules(st.draft);
      await invoke("harness_model_compacts_save", { agent, rules });
      st.saved = rules;
      st.draft = modelDraft(rules);
      for (const [key, other] of states) if (key.endsWith(` ${agent}`) && other !== st) states.delete(key);
    } catch (e) { st.error = String(e); }
    finally { st.saving = false; redraw(); }
  }
  function render(machine: string, agent: string): Node[] {
    const key = `${machine} ${agent}`;
    let st = states.get(key);

    if (!st) {
      st = { models: null, saved: [], draft: [], loading: false, saving: false, error: "", loaded: false, filter: "", defaultModel: "", savedDefault: "", savingDefault: false, defaultError: "" };
      states.set(key, st);
      void load(machine, agent, st);
    }

    const state = st, title = h("h4", "", "Models and context");
    if (st.loading) return [title, h("div", "hv-note", "Loading models and context limits…")];
    if (!st.loaded) return [title, h("div", "derr", st.error), button("Try again", "", () => { void load(machine, agent, state); redraw(); })];
    const defaultInput = h("input", "hv-in");
    const defaultModels = (st.models?.models ?? []).filter(model => model.id.toLowerCase() !== "default");
    defaultInput.value = defaultModels.find(model => model.id === st.defaultModel)?.name ?? st.defaultModel;
    defaultInput.placeholder = "Harness default";
    defaultInput.setAttribute("aria-label", "Default model");
    defaultInput.setAttribute("list", "harness-default-models");
    defaultInput.disabled = st.savingDefault;
    const choices = h("datalist", "");
    choices.id = "harness-default-models";

    for (const model of defaultModels) {
      const option = h("option", ""); option.value = model.name; choices.append(option);
    }

    const defaultError = h("span", "derr", st.defaultError);
    const saveDefault = button(st.savingDefault ? "Saving…" : "Save default model", "pri", () => {
      const model = state.defaultModel.trim();
      if (state.savingDefault || model === state.savedDefault) return;
      state.savingDefault = true; state.defaultError = ""; redraw();
      void invoke("harness_default_model_save", { agent, model: model || null }).then(() => {
        state.savedDefault = model; state.defaultModel = model;
        for (const [otherKey, other] of states) if (otherKey.endsWith(` ${agent}`) && other !== state) states.delete(otherKey);
      }).catch(e => { state.defaultError = String(e); }).finally(() => { state.savingDefault = false; redraw(); });
    });
    saveDefault.disabled = st.savingDefault || st.defaultModel.trim() === st.savedDefault;
    defaultInput.addEventListener("input", () => {
      const value = defaultInput.value.trim();
      state.defaultModel = defaultModels.find(model => model.name === value)?.id ?? value;
      saveDefault.disabled = state.savingDefault || state.defaultModel.trim() === state.savedDefault;
    });
    const defaultRow = h("div", "hv-f", h("span", "", h("b", "", "Default model"),
      h("small", "", "Used for new sessions on every machine. Blank uses the harness default.")),
      h("div", "", defaultInput, choices, saveDefault, defaultError));
    const editable = supportsOverrides(agent), models = st.models?.models ?? [];
    const note = h("div", "hv-note", "Limits from this machine's harness. Maximum usable can be smaller than the API limit.");
    const help = editable ? h("div", "hv-note", "Overrides apply to new or resumed sessions on every machine, between turns. Blank uses Auto-compact above; off disables Beep's trigger. The harness may compact earlier. Enter 80%, 350000, or off.") : null;
    const table = h("table", "fl-tbl model-context"), body = h("tbody", ""), problem = h("span", "derr", "");
    table.append(h("thead", "", h("tr", "", ...["Model", "Default usable", "Maximum usable", ...(editable ? ["Compact at"] : [])].map(t => h("th", "", t)))), body);
    const saveBtn = button(st.saving ? "Saving…" : "Save model overrides", "pri", () => void save(agent, state));
    const refresh = () => {
      problem.textContent = modelProblem(state.draft);
      saveBtn.disabled = state.saving || JSON.stringify(modelRules(state.draft)) === JSON.stringify(state.saved) || !!problem.textContent;
    };
    const ids = [...new Set([...models.map(m => m.id), ...st.draft.map(r => r.model)])];
    const limit = (n: number | null | undefined) => n ? n.toLocaleString("en-US") : "Not reported";
    const input = (value: string, label: string, placeholder: string, change: (v: string) => void) => {
      const i = h("input", "hv-in"); i.value = value; i.placeholder = placeholder; i.disabled = state.saving;
      i.setAttribute("aria-label", label); i.addEventListener("input", () => { change(i.value); refresh(); }); return i;
    };
    const filter = input(state.filter, "Filter models", "Filter models", value => {
      state.filter = value;
      for (const row of body.rows) row.hidden = !!row.dataset.model && !row.dataset.model.includes(value.toLowerCase());
    });

    for (const id of ids) {
      const model = models.find(m => m.id === id);
      const custom = state.draft.find(r => r.model === id);
      const cell = !model && custom && editable
        ? h("td", "", input(custom.model, "Model id for override", "Model id", v => { custom.model = v; }))
        : h("td", "", h("b", "", model?.name ?? id), model && model.name !== id ? h("small", "", id) : null);
      cell.title = model?.description ?? id;
      const row = h("tr", "", cell, h("td", "", limit(model?.context_window)), h("td", "", limit(model?.max_context_window)));
      if (editable) row.append(h("td", "", input(state.draft.find(r => r.model === id)?.threshold ?? "", `Compact at for ${id}`, "Harness setting", value => {
        const rule = custom ?? state.draft.find(r => r.model === id);
        if (rule) rule.threshold = value; else state.draft.push({ model: id, threshold: value });
      })));
      row.dataset.model = `${id} ${model?.name ?? ""}`.toLowerCase();
      row.hidden = !row.dataset.model.includes(state.filter.toLowerCase());
      body.append(row);
    }

    const add = button("Add model", "small", () => {
      const rule = { model: "", threshold: "" }; state.draft.push(rule);
      const idInput = input("", "Model id for override", "Model id", v => { rule.model = v; });
      body.append(h("tr", "", h("td", "", idInput), h("td", "", "Not reported"), h("td", "", "Not reported"), h("td", "", input("", "Compaction threshold for new model", "80% or 350000", v => { rule.threshold = v; }))));
      idInput.focus();
    });
    add.disabled = st.saving; refresh();
    return [title, defaultRow, note, ...(help ? [help] : []), ...(!ids.length ? [h("div", "hv-note", "This harness did not list its models.")] : []), filter, h("div", "model-context-scroll", table), ...(editable ? [h("div", "hv-acts", add, saveBtn, button("Discard", "quiet", () => { state.draft = modelDraft(state.saved); redraw(); }), problem)] : []), ...(st.error ? [h("div", "derr", st.error)] : [])];
  }
  return { render };
}
