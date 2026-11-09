import { useEffect, useSyncExternalStore, type ReactNode } from "react";
import { QUANTIZATIONS, SORTS, flshChanged, flshDraft, flshProblem, toggleQuantization, type FlshDraft, type Sort } from "./flsh-state.ts";
import type { FlshPanel } from "./flsh.ts";

const DEFAULT_TRIGGER = "Model's default trigger";

export function FlshView({ panel }: { panel: FlshPanel }) {
  const state = useSyncExternalStore(panel.subscribe, panel.snapshot);
  useEffect(() => { void panel.load(); }, [panel]);
  if (state.saved === null) return <><h4>OpenRouter routing</h4><div className="hv-note">Loading flsh's settings…</div></>;
  if (state.saved === false) return <><h4>OpenRouter routing</h4><div className="hv-note">The server needs an update before it keeps flsh's settings.</div></>;
  const draft = state.draft ?? flshDraft(state.saved);
  const set = (change: Partial<FlshDraft>) => panel.change(change);
  const problem = flshProblem(draft);
  const changed = flshChanged(state.saved, draft);
  const sorts: [string, Sort | null][] = [["Default", null], ...SORTS.map((s): [string, Sort | null] => [s[0].toUpperCase() + s.slice(1), s])];
  return <>
    <h4>OpenRouter routing</h4><div className="hv-note">Applies to every machine.</div>
    <Field label="Zero data retention" help="Only endpoints that keep no prompts."><Chips label="Zero data retention" values={[["Account setting", null], ["Require", true], ["Off", false]]} selected={v => draft.zdr === v} onPick={v => set({ zdr: v })} /></Field>
    <Field label="Allowed quantizations" help={draft.quantizations.length ? "Weight precisions providers may serve." : "None picked: any precision."}><Chips label="Allowed quantizations" className="fl-chips" values={QUANTIZATIONS.map(q => [q, q])} selected={v => draft.quantizations.includes(v)} onPick={v => set({ quantizations: toggleQuantization(draft, v) })} /></Field>
    <Field label="Sort providers by" help="Default: OpenRouter balances price and uptime."><Chips label="Sort providers by" values={sorts} selected={v => draft.sort === v} onPick={v => set({ sort: v })} /></Field>
    <Field label="Allow fallbacks" help="Try other providers when the first ones fail."><Chips label="Allow fallbacks" values={[["On (OpenRouter default)", null], ["Off", false]]} selected={v => (draft.allow_fallbacks === true ? null : draft.allow_fallbacks) === v} onPick={v => set({ allow_fallbacks: v })} /></Field>
    <h4>Auto-compact</h4><Field label="All models" help="Compact once the prompt passes this many tokens."><span className="fl-row"><Switch on={draft.compact_on} label="Auto-compact for all models" flip={() => set({ compact_on: !draft.compact_on })} /><Input value={draft.compact_tokens} placeholder={draft.compact_on ? DEFAULT_TRIGGER : "Off: waits for the window to overflow"} label="Auto-compact threshold" change={v => set({ compact_tokens: v })} /></span></Field>
    <table className="fl-tbl"><tbody><tr><th>Model</th><th>On</th><th>Threshold (tokens)</th><th /></tr>{draft.models.map((m, i) => {
      const change = (value: Partial<typeof m>) => set({ models: draft.models.map((model, index) => index === i ? { ...model, ...value } : model) });
      return <tr key={i}><td><Input value={m.model} placeholder="provider:model" label={`Model ${i + 1}`} change={v => change({ model: v })} /></td><td><Switch on={m.enabled} label={`Auto-compact for model ${i + 1}`} flip={() => change({ enabled: !m.enabled })} /></td><td><Input value={m.tokens} placeholder={m.enabled ? DEFAULT_TRIGGER : "Off"} label={`Threshold for model ${i + 1}`} change={v => change({ tokens: v })} /></td><td><button type="button" className="dbtn quiet small" aria-label={`Remove ${m.model}`} onClick={() => set({ models: draft.models.filter((_, index) => index !== i) })}>×</button></td></tr>;
    })}<tr><td><span className="hv-note">{draft.models.length ? "" : "No model has its own rule."}</span></td><td /><td /><td><button type="button" className="dbtn small" onClick={() => set({ models: [...draft.models, { model: "", enabled: true, tokens: "" }] })}>Add model</button></td></tr></tbody></table>
    <div className="hv-acts"><button type="button" className="dbtn pri" disabled={state.saving || !changed || !!problem} onClick={() => void panel.save()}>{state.saving ? "Saving…" : "Save"}</button><button type="button" className="dbtn quiet" hidden={!changed} onClick={panel.discard}>Discard</button><span className="derr">{problem || state.error}</span></div>
  </>;
}

function Field({ label, help, children }: { label: string; help: string; children: ReactNode }) { return <div className="hv-f"><span><b>{label}</b><small>{help}</small></span>{children}</div>; }
function Chips<T>({ label, values, selected, onPick, className = "fl-seg" }: { label: string; values: readonly (readonly [string, T])[]; selected: (value: T) => boolean; onPick: (value: T) => void; className?: string }) { return <span className={className} role="group" aria-label={label}>{values.map(([text, value]) => <button type="button" className="dbtn dchip" key={text} aria-pressed={selected(value)} onClick={() => onPick(value)}>{text}</button>)}</span>; }
function Switch({ on, label, flip }: { on: boolean; label: string; flip: () => void }) { return <button type="button" className="dswitch" role="switch" aria-checked={on} aria-label={label} onClick={flip} />; }
function Input({ value, placeholder, label, change }: { value: string; placeholder: string; label: string; change: (value: string) => void }) { return <input className="hv-in" value={value} placeholder={placeholder} aria-label={label} spellCheck={false} autoComplete="off" onChange={e => change(e.currentTarget.value)} />; }
