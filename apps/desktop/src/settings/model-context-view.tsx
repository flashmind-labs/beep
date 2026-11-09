import { useEffect, useRef, useSyncExternalStore } from "react";
import { modelDraft, modelProblem, modelRules } from "./model-context-state.ts";
import { supportsOverrides, type ModelContextPanel } from "./model-context.ts";

export function ModelContextView({ panel, machine, agent }: { panel: ModelContextPanel; machine: string; agent: string }) {
  const state = useSyncExternalStore(panel.subscribe, () => panel.snapshot(machine, agent));
  const added = useRef<HTMLInputElement>(null);
  const focusAdded = useRef(false);
  useEffect(() => { if (!state.loaded && !state.loading && !state.error) void panel.load(machine, agent); }, [panel, machine, agent, state.loaded, state.loading, state.error]);
  useEffect(() => { if (focusAdded.current && added.current) { added.current.focus(); focusAdded.current = false; } }, [state.draft]);
  const update = (change: Parameters<typeof panel.update>[2]) => panel.update(machine, agent, change);
  if (state.loading || (!state.loaded && !state.error)) return <><h4>Models and context</h4><div className="hv-note">Loading models and context limits…</div></>;
  if (!state.loaded) return <><h4>Models and context</h4><div className="derr">{state.error}</div><button type="button" className="dbtn" onClick={() => void panel.load(machine, agent)}>Try again</button></>;
  const models = state.models?.models ?? [], defaultModels = models.filter(model => model.id.toLowerCase() !== "default");
  const editable = supportsOverrides(agent), problem = modelProblem(state.draft);
  const rows = [...models.map(model => ({ id: model.id, index: state.draft.findIndex((rule, i) => rule.model === model.id && !state.customIndexes.includes(i)), model })), ...state.customIndexes.map(index => ({ id: state.draft[index]?.model ?? "", index, model: null }))];
  const limit = (n: number | null | undefined) => n ? n.toLocaleString("en-US") : "Not reported";
  const changeRule = (index: number, change: { model?: string; threshold?: string }) => update({ draft: state.draft.map((rule, i) => i === index ? { ...rule, ...change } : rule) });
  return <><h4>Models and context</h4>
    <div className="hv-f"><span><b>Default model</b><small>Used for new sessions on every machine. Blank uses the harness default.</small></span><div>
      <input className="hv-in" aria-label="Default model" list="harness-default-models" placeholder="Harness default" disabled={state.savingDefault} value={state.defaultText} onChange={e => { const defaultText = e.currentTarget.value, value = defaultText.trim(); update({ defaultText, defaultModel: defaultModels.find(model => model.name === value)?.id ?? value }); }} />
      <datalist id="harness-default-models">{defaultModels.map(model => <option key={model.id} value={model.name} />)}</datalist>
      <button type="button" className="dbtn pri" disabled={state.savingDefault || state.defaultModel.trim() === state.savedDefault} onClick={() => void panel.saveDefault(machine, agent)}>{state.savingDefault ? "Saving…" : "Save default model"}</button><span className="derr">{state.defaultError}</span>
    </div></div>
    <div className="hv-note">Limits from this machine's harness. Maximum usable can be smaller than the API limit.</div>
    {editable && <div className="hv-note">Overrides apply to new or resumed sessions on every machine, between turns. Blank uses Auto-compact above; off disables Beep's trigger. The harness may compact earlier. Enter 80%, 350000, or off.</div>}
    {!rows.length && <div className="hv-note">This harness did not list its models.</div>}
    <input className="hv-in" aria-label="Filter models" placeholder="Filter models" disabled={state.saving} value={state.filter} onChange={e => update({ filter: e.currentTarget.value, unfilteredIndexes: [] })} />
    <div className="model-context-scroll"><table className="fl-tbl model-context"><thead><tr><th>Model</th><th>Default usable</th><th>Maximum usable</th>{editable && <th>Compact at</th>}</tr></thead><tbody>
      {rows.map(({ id, index, model }) => {
        const custom = index >= 0 ? state.draft[index] : null;
        return <tr key={model ? `model:${id}` : `draft:${index}`} data-model={`${id} ${model?.name ?? ""}`.toLowerCase()} hidden={!state.unfilteredIndexes.includes(index) && !`${id} ${model?.name ?? ""}`.toLowerCase().includes(state.filter.toLowerCase())}>
          <td title={model?.description ?? id}>{!model && custom && editable ? <input ref={index === state.draft.length - 1 ? added : null} className="hv-in" aria-label="Model id for override" placeholder="Model id" disabled={state.saving} value={custom.model} onChange={e => changeRule(index, { model: e.currentTarget.value })} /> : <><b>{model?.name ?? id}</b>{model && model.name !== id && <small>{id}</small>}</>}</td>
          <td>{limit(model?.context_window)}</td><td>{limit(model?.max_context_window)}</td>
          {editable && <td><input className="hv-in" aria-label={!model && id === "" ? "Compaction threshold for new model" : `Compact at for ${id}`} placeholder={!model && id === "" ? "80% or 350000" : "Harness setting"} disabled={state.saving} value={custom?.threshold ?? ""} onChange={e => { const threshold = e.currentTarget.value; if (index >= 0) changeRule(index, { threshold }); else update({ draft: [...state.draft, { model: id, threshold }] }); }} /></td>}
        </tr>;
      })}
    </tbody></table></div>
    {editable && <div className="hv-acts"><button type="button" className="dbtn small" disabled={state.saving} onClick={() => { focusAdded.current = true; update({ draft: [...state.draft, { model: "", threshold: "" }], customIndexes: [...state.customIndexes, state.draft.length], unfilteredIndexes: [...state.unfilteredIndexes, state.draft.length] }); }}>Add model</button><button type="button" className="dbtn pri" disabled={state.saving || JSON.stringify(modelRules(state.draft)) === JSON.stringify(state.saved) || !!problem} onClick={() => void panel.save(machine, agent)}>{state.saving ? "Saving…" : "Save model overrides"}</button><button type="button" className="dbtn quiet" onClick={() => update({ draft: modelDraft(state.saved), customIndexes: state.saved.flatMap((rule, i) => models.some(model => model.id === rule.model) ? [] : [i]), unfilteredIndexes: [] })}>Discard</button><span className="derr">{problem}</span></div>}
    {state.error && <div className="derr">{state.error}</div>}
  </>;
}
