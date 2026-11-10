import { useEffect, type ReactNode } from "react";
import { ModelContextView } from "./model-context-view.tsx";
import type { ModelContextPanel } from "./model-context.ts";
import { FlshView } from "./flsh-view.tsx";
import type { FlshPanel } from "./flsh.ts";
import { copy } from "../ui/parts.ts";
import { splitFailure } from "../ui/failbanner-view.tsx";
import { HarnessIcon } from "../ui/icons.tsx";
import { adapterLine, type AdapterInstall } from "../machines/adapters-state.ts";
import { compactKind, differsLabel, differsOn, draftChanged, envProblem, harnessName, hasUpdate, listRows, planOf, summaryLine, versionLine, type AutoCompactDraft, type HarnessDraft, type HarnessesView, type HarnessView, type MachineSettings, type SharedField } from "./providers-state.ts";

export interface SharedProviderSettings {
  agent: string; machineName: string | null;
  compact: { loaded: boolean; draft: AutoCompactDraft; busy: boolean; edited: boolean; error: string; canSave: boolean } | null;
}

interface ProviderProps {
  machines: { id: string; name: string; online: boolean }[] | null | undefined;
  machine: string | null; machineName: string; view: HarnessesView | null | false | undefined; picked: string | null;
  draft: HarnessDraft | null; others: MachineSettings[] | false | undefined; saving: boolean; updating: boolean;
  adapter: AdapterInstall | null | undefined; error: string; inSettings: boolean; savedAll: string; includeProgram: boolean;
  shared: SharedProviderSettings | null;
  loadOthers(agent: string): void; loadCompact(agent: string): void; changeCompact(agent: string, change: Partial<AutoCompactDraft>): void; saveCompact(agent: string): void; discardCompact(agent: string): void;
  flsh: FlshPanel; contexts: ModelContextPanel;
  loading: boolean; checked: string; updateCount: number; canUpdate: boolean;
  pick: (agent: string) => void; toggle: (hv: HarnessView, on: boolean) => void; update: (hv: HarnessView) => void;
  retryAdapter: (agent: string) => void; openAccounts: () => void; setDraft: (value: Partial<HarnessDraft>) => void;
  save: (hv: HarnessView) => void; saveAll: (hv: HarnessView) => void; discard: () => void;
  setIncludeProgram: (value: boolean) => void; dismissError: () => void;
  pickMachine: (id: string) => void; refresh: () => void; updateAll: () => void;
}

export function ProvidersView(p: ProviderProps) {
  const active = p.view ? p.view.harnesses.find(item => item.agent === p.picked) : null;
  const agent = active?.installed && p.others === undefined ? active.agent : null;
  useEffect(() => { if (agent) p.loadOthers(agent); }, [agent, p.loadOthers]);
  return <div className={`cc-view hv-view${p.inSettings ? " in-set" : ""}`}>
    {p.inSettings ? <>{p.shared && <SharedSettings value={p.shared} p={p} />}<section className="hv-scope"><Toolbar p={p} /><Body {...p} /></section></> : <Body {...p} />}
  </div>;
}

function Body(p: ProviderProps) {
  if (p.machines === undefined) return <Empty title="Loading your machines…" />;
  if (p.machines === null) return <div className="cc-err">{p.error || "Could not load your machines."}</div>;
  if (!p.machine) return <Empty title="No machine online" sub="Coding agents show here for machines that are online." />;
  if (p.view === undefined) return <Empty title={`Asking ${p.machineName}…`} />;
  if (p.view === false) return <div className="cc-err">The server needs an update before it shows coding agents here.</div>;
  const hv = p.view?.harnesses.find(h => h.agent === p.picked);
  const failure = splitFailure(p.error);
  const error = p.error && <div className="fb-err" role="alert"><div className="fb-line"><span>{failure.line}</span><button type="button" className="fb-x" aria-label="Dismiss" title="Dismiss" onClick={p.dismissError}>×</button></div>{failure.details && <details className="fb-more"><summary>Details</summary><pre>{failure.details}</pre></details>}</div>;
  if (p.view === null) return error || <div className="cc-err">Could not read the coding agents.</div>;
  if (p.inSettings) return <>{error}{hv ? <><Summary hv={hv} p={p} /><Runtime hv={hv} p={p} /></> : <Empty title={`${p.machineName} does not report ${harnessName(p.picked ?? "")}`} />}</>;
  return <>{error}<div className="hv"><AgentList view={p.view} p={p} />{hv ? <div>{p.shared && <SharedSettings value={p.shared} p={p} />}<Runtime key={`${p.machine}:${hv.agent}`} hv={hv} p={p} /></div> : <div className="hv-d" />}</div></>;
}

function Toolbar({ p }: { p: ProviderProps }) {
  const machines = p.machines ?? [];
  return <div className="hv-bar"><span className="hv-pick crowded">
    <span className="hv-seg" role="group" aria-label="Machine">{machines.map(m => <button key={m.id} type="button" title={m.online ? m.name : `${m.name} (offline)`} aria-pressed={m.id === p.machine} disabled={!m.online} onClick={() => p.pickMachine(m.id)}>{m.name}</button>)}</span>
    <select className="hv-sel" value={p.machine ?? ""} aria-label="Machine runtime settings" onChange={e => p.pickMachine(e.currentTarget.value)}>{machines.map(m => <option key={m.id} value={m.id} disabled={!m.online}>{m.online ? m.name : `${m.name} (offline)`}</option>)}</select>
  </span><span className="sp" /><button type="button" className="dbtn quiet" title="Check the versions again" disabled={p.loading || !p.machine} onClick={p.refresh}>{p.loading ? "Checking…" : p.checked}</button><button type="button" className="dbtn" disabled={!!p.updateCount || !p.canUpdate} title={!p.canUpdate && !p.updateCount ? "Everything is up to date" : undefined} onClick={p.updateAll}>{p.updateCount ? `Updating ${p.updateCount}…` : "Update all"}</button></div>;
}

function Empty({ title, sub }: { title: string; sub?: string }) { return <div className="cc-panel cc-empty"><b>{title}</b>{sub && <span>{sub}</span>}</div>; }
function SharedSettings({ value, p }: { value: SharedProviderSettings; p: ProviderProps }) {
  const compact = value.compact;
  useEffect(() => { if (compact && !compact.loaded) p.loadCompact(value.agent); }, [value.agent, compact?.loaded, p.loadCompact]);
  return <section className="hv-scope hv-d"><h3>All machines</h3><div className="hv-note">These settings apply on every machine.</div>
    {compact && <><div className="hv-f"><span><b>Auto-compact</b><small>Applies to sessions on every machine.</small></span><span className="fl-row">
      <button type="button" className="dswitch" role="switch" aria-label={`Auto-compact for ${harnessName(value.agent)}`} aria-checked={compact.draft.compactOn} disabled={!compact.loaded || compact.busy} onClick={() => p.changeCompact(value.agent, { compactOn: !compact.draft.compactOn })} />
      {compactKind(value.agent) === "percent" && <input className="hv-in" value={compact.draft.compactPercent} placeholder="80% or 350000" aria-label="Auto-compact threshold" autoComplete="off" spellCheck={false} disabled={!compact.loaded || compact.busy} onChange={event => p.changeCompact(value.agent, { compactPercent: event.currentTarget.value })} />}
    </span></div><div className="hv-acts"><button type="button" className="dbtn pri" disabled={!compact.canSave} onClick={() => p.saveCompact(value.agent)}>{compact.busy ? "Saving…" : "Save auto-compact"}</button><button type="button" className="dbtn quiet" hidden={!compact.edited} disabled={compact.busy} onClick={() => p.discardCompact(value.agent)}>Discard</button><span className="derr">{compact.error}</span></div></>}
    {value.agent === "flsh" && <FlshView panel={p.flsh} />}
    {value.machineName && <div className="hv-note">Model choices and reported limits come from {value.machineName}.</div>}
    {p.machine && <ModelContextView panel={p.contexts} machine={p.machine} agent={value.agent} />}
  </section>;
}
function Switch({ hv, p, on }: { hv: HarnessView; p: ProviderProps; on: boolean }) {
  return <button type="button" className="dswitch" role="switch" aria-checked={on} aria-label={`${harnessName(hv.agent)} on ${p.machineName}`} disabled={p.saving} onClick={e => { e.stopPropagation(); p.toggle(hv, !on); }} onKeyDown={e => e.stopPropagation()} />;
}
function Name({ name, version, latest, status, bad }: { name: string; version: string; latest: string | null; status: string; bad: boolean }) {
  return <span className="hv-name"><span className="hv-nm"><b>{name}</b>{version && <span className="ver">{version}</span>}{latest && <span className="hv-up" title={`v${latest} is out`}>update</span>}</span><small className={bad ? "bad" : ""} title={status}>{status}</small></span>;
}
function AgentList({ view, p }: { view: HarnessesView; p: ProviderProps }) {
  return <div className="hv-list">{listRows(view).map(r => { const hv = view.harnesses.find(h => h.agent === r.agent); if (!hv) return null; return <div key={r.agent} className={`hv-it${r.on || !r.switchable ? "" : " off"}`} role="button" tabIndex={0} aria-current={r.agent === p.picked} onClick={() => p.pick(r.agent)} onKeyDown={e => { if (e.target === e.currentTarget && (e.key === "Enter" || e.key === " ")) { e.preventDefault(); p.pick(r.agent); } }}><HarnessIcon agent={r.agent} /><Name name={r.name} version={r.version ? `v${r.version}` : ""} latest={r.update ? hv.latest : null} status={r.status} bad={r.bad} />{r.switchable ? <Switch hv={hv} p={p} on={r.on} /> : <span />}</div>; })}</div>;
}
function Summary({ hv, p }: { hv: HarnessView; p: ProviderProps }) {
  const s = p.adapter ? { text: adapterLine(p.adapter), bad: p.adapter.failed !== null } : summaryLine(hv);
  const on = hv.installed && hv.settings.enabled;
  return <div className={`hv-sum${hv.installed && !on ? " off" : ""}`}><HarnessIcon agent={hv.agent} /><Name name={harnessName(hv.agent)} version={hv.installed && hv.version ? `v${hv.version}` : ""} latest={hasUpdate(hv) ? hv.latest : null} status={s.text} bad={s.bad} />{hv.installed ? <><span className="on">{on ? "On" : "Off"} for {p.machineName}</span><Switch hv={hv} p={p} on={on} /></> : <><span /><span /></>}</div>;
}
function Field({ label, help, names, children }: { label: string; help: string; names?: readonly string[]; children: ReactNode }) {
  return <div className="hv-f"><span><b>{label}</b>{!!names?.length && <span className="hv-diff" title={`Not the same on ${names.join(", ")}`}>{differsLabel(names)}</span>}<small>{help}</small></span>{children}</div>;
}
function Runtime({ hv, p }: { hv: HarnessView; p: ProviderProps }) {
  const draft = p.draft, name = harnessName(hv.agent), pending = p.adapter, busy = p.updating;
  const field = (label: SharedField) => p.others && p.machine ? differsOn(p.others, p.machine, label) : [];
  const status = hv.sign_in;
  return <div className="hv-d">
    {!p.inSettings && <><h3><HarnessIcon agent={hv.agent} />{name}<span className="ver">{versionLine(hv)}</span></h3><h4>Status</h4></>}
    {!hv.installed ? <>{!p.inSettings && <div className="hv-stat">Not installed on {p.machineName}.</div>}{hv.install ? <div className="hv-acts"><button type="button" className="dbtn pri" disabled={busy} onClick={() => p.update(hv)}>{busy ? "Installing…" : `Install on ${p.machineName}`}</button><button type="button" className="dbtn quiet" onClick={e => void copy(hv.install ?? "", e.currentTarget)}>Copy install command</button></div> : <div className="hv-note">Install it on the machine, then check again.</div>}</> : <>
      {!p.inSettings && (status.state === "signed_in" ? <div className="hv-stat"><span className="ddot" />{`Signed in${status.account ? ` as ${status.account}` : ""}${planOf(status)}`}</div> : status.state === "beep_token" ? <div className="hv-stat"><span className="ddot" />{`Signs in with the token in Agent accounts (…${status.last4 ?? ""})`}</div> : status.state === "signed_out" ? <div className="hv-stat"><span className="ddot" style={{ background: "var(--hot)" }} />Not signed in on {p.machineName}</div> : <div className="hv-stat">{name} does not say whether it is signed in. Run it on {p.machineName} to sign in.</div>)}
      {status.state === "beep_token" && <div className="hv-acts"><button type="button" className="dbtn" onClick={p.openAccounts}>Agent accounts</button></div>}
      {status.state === "signed_out" && (hv.agent === "claude" || hv.agent === "codex" ? <div className="hv-acts"><button type="button" className="dbtn pri" onClick={p.openAccounts}>Sign in</button></div> : <div className="hv-note">Run {hv.agent} on {p.machineName} and sign in there.</div>)}
      {status.state === "unknown" && p.inSettings && <div className="hv-note">{name} does not say whether it is signed in. Run it on {p.machineName} to sign in.</div>}
      {pending ? <>{(!p.inSettings || pending.failed !== null) && <div className={`hv-note${pending.failed === null ? "" : " bad"}`}>{adapterLine(pending)}</div>}{pending.failed !== null && <div className="hv-acts"><button type="button" className="dbtn" disabled={busy} onClick={() => p.retryAdapter(hv.agent)}>{busy ? "Trying again…" : "Try again"}</button></div>}</> : !hv.runnable && <div className="hv-note">Restart beep-node to use it.</div>}
      {hasUpdate(hv) && <div className="hv-acts"><button type="button" className="dbtn" disabled={busy || !hv.install} title={!hv.install ? "Beep knows no way to update it here" : undefined} onClick={() => p.update(hv)}>{busy ? "Updating…" : `Update to v${hv.latest}`}</button></div>}
      {draft && <><h4>Runtime</h4><Field label="Binary path" help={`The command Beep starts for ${name} on ${p.machineName}.`} names={field("program")}><TextInput label="Binary path" placeholder={hv.program ?? hv.agent} value={draft.program} change={v => p.setDraft({ program: v })} /></Field><Field label={hv.home_var ?? "Home folder"} help="Its home and config folder." names={field("home")}><TextInput label={hv.home_var ?? "Home folder"} placeholder={hv.home_default ?? ""} value={draft.home} change={v => p.setDraft({ home: v })} /></Field><Field label="Launch arguments" help="Added to every session it starts." names={field("args")}><TextInput label="Launch arguments" placeholder="none" value={draft.args} change={v => p.setDraft({ args: v })} /></Field>
      <Field label="Environment" help="Encrypted, never shown again. Put keys in Credentials." names={field("env")}><div className="hv-envs">{[...draft.env, { name: "", value: "", saved: false }].map((v, i) => { const last = i === draft.env.length; const set = (change: Partial<typeof v>) => { const env = [...draft.env]; if (last) env.push({ ...v, ...change }); else env[i] = { ...v, ...change }; p.setDraft({ env }); }; return <div className="hv-env" key={i}><TextInput label="Variable name" placeholder="NAME" value={v.name} change={value => set({ name: value })} /><TextInput type="password" label="Value" placeholder={v.saved ? "saved; type to replace" : "value"} value={v.value} change={value => set({ value })} />{last ? <span /> : <button type="button" className="dbtn quiet small" aria-label={`Remove ${v.name}`} onClick={() => p.setDraft({ env: draft.env.filter((_, j) => j !== i) })}>×</button>}</div>; })}</div></Field>
      <div className="hv-acts hv-save"><button type="button" className="dbtn pri" title={`Save for ${p.machineName} only`} disabled={p.saving || !draftChanged(hv, draft) || !!envProblem(draft.env)} onClick={() => p.save(hv)}>{p.saving ? "Saving…" : "Save"}</button>{p.others && p.others.length > 1 && <><button type="button" className="dbtn" title="Save these settings on every machine; offline ones get them when they next connect" disabled={p.saving || !!envProblem(draft.env)} onClick={() => p.saveAll(hv)}>{p.saving ? "Saving…" : "Save to all hosts"}</button><label className="hv-incl" title="Binary paths usually differ between machines"><input type="checkbox" checked={p.includeProgram} onChange={e => p.setIncludeProgram(e.currentTarget.checked)} />Include binary path</label></>}<button type="button" className="dbtn quiet" hidden={!draftChanged(hv, draft)} onClick={p.discard}>Discard</button><span className="hv-ok">{draftChanged(hv, draft) ? "" : p.savedAll}</span><span className="derr">{envProblem(draft.env)}</span></div></>}
    </>}
  </div>;
}
function TextInput({ value, label, placeholder, change, type = "text" }: { value: string; label: string; placeholder: string; change: (value: string) => void; type?: "text" | "password" }) {
  return <input type={type} className="hv-in" value={value} placeholder={placeholder} spellCheck={false} autoComplete="off" aria-label={label} onChange={e => change(e.currentTarget.value)} />;
}

export function ProviderHeader(p: Pick<ProviderProps, "machines" | "machine" | "loading" | "checked" | "updateCount" | "canUpdate" | "pickMachine" | "refresh" | "updateAll">) {
  const machines = p.machines ?? [];
  const crowded = machines.length > 4 || machines.reduce((sum, machine) => sum + machine.name.length, 0) > 48;
  return <><span className={`hv-pick${crowded ? " crowded" : ""}`}><span className="hv-seg" role="group" aria-label="Machine">{machines.map(machine => <button type="button" key={machine.id} title={machine.online ? machine.name : `${machine.name} (offline)`} aria-pressed={machine.id === p.machine} disabled={!machine.online} onClick={() => p.pickMachine(machine.id)}>{machine.name}</button>)}</span><select className="hv-sel" value={p.machine ?? ""} aria-label="Machine" onChange={event => p.pickMachine(event.currentTarget.value)}>{machines.map(machine => <option key={machine.id} value={machine.id} disabled={!machine.online}>{machine.online ? machine.name : `${machine.name} (offline)`}</option>)}</select></span><button type="button" className="dbtn" disabled={!!p.updateCount || !p.canUpdate} title={!p.canUpdate && !p.updateCount ? "Everything is up to date" : undefined} onClick={p.updateAll}>{p.updateCount ? `Updating ${p.updateCount}…` : "Update all"}</button><button type="button" className="dbtn quiet" disabled={p.loading || !p.machine} title="Check the versions again" onClick={p.refresh}>{p.loading ? "Checking…" : p.checked}</button></>;
}
