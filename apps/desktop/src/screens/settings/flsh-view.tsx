import { useEffect, type ReactNode } from "react";
import { useStore } from "zustand";
import { NEW_MODEL, QUANTIZATIONS, SORTS, flshChanged, flshDraft, flshProblem, toggleQuantization, type FlshDraft, type ModelCompact, type ModelDraft, type RoutingAgent, type Sort } from "../../stores/settings/flsh-state.ts";
import type { FlshPanel } from "../../stores/settings/flsh.ts";
import { cap } from "../../ui/format.ts";
import { Button, NO_AUTO, Switch } from "../../ui/kit.tsx";

const DEFAULT_TRIGGER = "Model's default trigger";
const MODEL_COMPACT: [string, ModelCompact][] = [["All models", "all"], ["On", "on"], ["Off", "off"]];

export function FlshView({ panel, agent }: { panel: FlshPanel; agent: RoutingAgent }) {
  const state = useStore(panel.state);
  useEffect(() => { void panel.load(); }, [panel]);
  if (state.saved === null) return <><h4>OpenRouter routing</h4><div className="hv-note">Loading flsh's settings…</div></>;
  const draft = state.draft ?? flshDraft(state.saved);
  const set = (change: Partial<FlshDraft>) => panel.change(change);
  const problem = flshProblem(draft);
  const changed = flshChanged(state.saved, draft);
  const sorts: [string, Sort | null][] = [["Default", null], ...SORTS.map((s): [string, Sort | null] => [cap(s), s])];
  const flsh = agent === "flsh";
  const models = (models: ModelDraft[]) => set({ models });
  return <>
    <h4>OpenRouter routing</h4><div className="hv-note">Applies to flsh and pi on every machine.</div>
    <Field label="Zero data retention" help="Only endpoints that keep no prompts."><Chips label="Zero data retention" values={[["Account setting", null], ["Require", true], ["Off", false]]} selected={v => draft.zdr === v} onPick={v => set({ zdr: v })} /></Field>
    <Field label="Allowed quantizations" help={draft.quantizations.length ? "Weight precisions providers may serve." : "None picked: any precision."}><Chips label="Allowed quantizations" className="fl-chips" values={QUANTIZATIONS.map(q => [q, q])} selected={v => draft.quantizations.includes(v)} onPick={v => set({ quantizations: toggleQuantization(draft.quantizations, v) })} /></Field>
    <Field label="Sort providers by" help="Default: OpenRouter balances price and uptime."><Chips label="Sort providers by" values={sorts} selected={v => draft.sort === v} onPick={v => set({ sort: v })} /></Field>
    <Field label="Allow fallbacks" help="Try other providers when the first ones fail."><Chips label="Allow fallbacks" values={[["On (OpenRouter default)", null], ["Off", false]]} selected={v => (draft.allow_fallbacks === true ? null : draft.allow_fallbacks) === v} onPick={v => set({ allow_fallbacks: v })} /></Field>
    {flsh && <Field label="Stream idle timeout (seconds)" help="How long flsh waits between OpenRouter updates. Leave blank to use the host setting, or 300 seconds on a new host."><Input numeric label="Stream idle timeout (seconds)" placeholder="300" value={draft.idle_seconds} change={v => set({ idle_seconds: v })} /></Field>}
    {flsh && <><h4>Auto-compact</h4><Field label="All models" help="Compact once the prompt passes this many tokens. flsh only."><span className="fl-row"><Switch on={draft.compact_on} label="Auto-compact for all models" flip={() => set({ compact_on: !draft.compact_on })} /><Input value={draft.compact_tokens} placeholder={draft.compact_on ? DEFAULT_TRIGGER : "Off: waits for the window to overflow"} label="Auto-compact threshold" change={v => set({ compact_tokens: v })} /></span></Field></>}
    <h4>Per model</h4><div className="hv-note">Settings that replace the ones above for one model.</div>
    <table className="fl-tbl"><tbody><tr><th>Model</th>{flsh && <><th className="fl-c">Auto-compact</th><th className="fl-t">Threshold (tokens)</th></>}<th className="fl-x" /></tr>{draft.models.map((m, i) => <ModelRow key={i} m={m} n={i + 1} flsh={flsh} change={value => models(draft.models.map((model, index) => index === i ? { ...model, ...value } : model))} remove={() => models(draft.models.filter((_, index) => index !== i))} />)}
      <tr><td><span className="hv-note">{draft.models.length ? "" : "No model has its own settings."}</span></td>{flsh && <><td /><td /></>}<td><Button small onClick={() => models([...draft.models, NEW_MODEL])}>Add model</Button></td></tr></tbody></table>
    <div className="hv-acts"><Button look="primary" disabled={state.saving || !changed || !!problem} onClick={() => void panel.save()}>{state.saving ? "Saving…" : "Save"}</Button><Button look="quiet" hidden={!changed} onClick={panel.discard}>Discard</Button><span className="derr">{problem || state.error}</span></div>
  </>;
}

/// One model's settings: its name and compaction on the first line, its
/// quantizations under them.
function ModelRow({ m, n, flsh, change, remove }: { m: ModelDraft; n: number; flsh: boolean; change: (value: Partial<ModelDraft>) => void; remove: () => void }) {
  const own: [string, string[] | null][] = [["All models", null], ["Any", []]];
  return <>
    <tr className="fl-head"><td><Input value={m.model} placeholder="openrouter:vendor/model" label={`Model ${n}`} change={v => change({ model: v })} /></td>
      {flsh && <><td><Chips label={`Auto-compact for model ${n}`} values={MODEL_COMPACT} selected={v => m.compact === v} onPick={v => change({ compact: v })} /></td>
        <td>{m.compact !== "all" && <Input value={m.tokens} placeholder={m.compact === "on" ? DEFAULT_TRIGGER : "Off"} label={`Threshold for model ${n}`} change={v => change({ tokens: v })} />}</td></>}
      <td><Button look="quiet" small aria-label={`Remove ${m.model || `model ${n}`}`} onClick={remove}>×</Button></td></tr>
    <tr><td colSpan={flsh ? 4 : 2}><span className="fl-row"><small>Quantizations</small>
      <Chips label={`Quantizations for model ${n}`} values={own} selected={v => v === null ? m.quantizations === null : m.quantizations?.length === 0} onPick={v => change({ quantizations: v })} />
      <Chips label={`Allowed quantizations for model ${n}`} className="fl-chips" values={QUANTIZATIONS.map(q => [q, q])} selected={v => m.quantizations?.includes(v) ?? false} onPick={v => change({ quantizations: toggleQuantization(m.quantizations ?? [], v) })} /></span></td></tr>
  </>;
}

function Field({ label, help, children }: { label: string; help: string; children: ReactNode }) { return <div className="hv-f"><span><b>{label}</b><small>{help}</small></span>{children}</div>; }
function Chips<T>({ label, values, selected, onPick, className = "fl-seg" }: { label: string; values: readonly (readonly [string, T])[]; selected: (value: T) => boolean; onPick: (value: T) => void; className?: string }) { return <span className={className} role="group" aria-label={label}>{values.map(([text, value]) => <Button className="dchip" key={text} aria-pressed={selected(value)} onClick={() => onPick(value)}>{text}</Button>)}</span>; }
function Input({ value, placeholder, label, change, numeric = false }: { value: string; placeholder: string; label: string; numeric?: boolean; change: (value: string) => void }) { return <input className="hv-in" inputMode={numeric ? "numeric" : "text"} value={value} placeholder={placeholder} aria-label={label} {...NO_AUTO} onChange={e => change(e.currentTarget.value)} />; }
