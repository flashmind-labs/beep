import { useEffect, useState, type ReactNode } from "react";
import { ModelContextView } from "./model-context-view.tsx";
import type { ModelContextPanel } from "../../stores/settings/model-context.ts";
import { FlshView } from "./flsh-view.tsx";
import type { FlshPanel } from "../../stores/settings/flsh.ts";
import { FailBanner } from "../../ui/failbanner-view.tsx";
import { HarnessIcon } from "../../ui/icons.tsx";
import { adapterLine, type AdapterInstall } from "../../stores/machines/adapters-state.ts";
import { compactKind, differsLabel, differsOn, draftChanged, envProblem, harnessName, hasUpdate, listRows, planOf, summaryLine, versionLine, type AutoCompactDraft, type HarnessDraft, type HarnessesView, type HarnessView, type MachineSettings, type SharedField } from "../../stores/settings/providers-state.ts";
import { Button, Checkbox, ChipToggle, CopyButton, EmptyPanel, NO_AUTO, onRowKey, Picker, Switch } from "../../ui/kit.tsx";
import type { invoke as Invoke } from "../../ui/tauri.ts";

export interface SharedProviderSettings {
  agent: string; machineName: string | null;
  compact: { loaded: boolean; draft: AutoCompactDraft; busy: boolean; edited: boolean; error: string; canSave: boolean } | null;
}

interface ProviderProps {
  machines: { id: string; name: string; online: boolean }[] | null | undefined;
  machine: string | null; machineName: string; view: HarnessesView | null | undefined; picked: string | null;
  draft: HarnessDraft | null; others: MachineSettings[] | false | undefined; saving: boolean; updating: boolean;
  adapter: AdapterInstall | null | undefined; error: string; inSettings: boolean; savedAll: string; includeProgram: boolean;
  shared: SharedProviderSettings | null;
  loadOthers(agent: string): void; loadCompact(agent: string): void; changeCompact(agent: string, change: Partial<AutoCompactDraft>): void; saveCompact(agent: string): void; discardCompact(agent: string): void;
  flsh: FlshPanel; contexts: ModelContextPanel; invoke: typeof Invoke;
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
  if (p.machines === undefined) return <EmptyPanel title="Loading your machines…" />;
  if (p.machines === null) return <div className="cc-err">{p.error || "Could not load your machines."}</div>;
  if (!p.machine) return <EmptyPanel title="No machine online" text="Coding agents show here for machines that are online." />;
  if (p.view === undefined) return <EmptyPanel title={`Asking ${p.machineName}…`} />;
  const hv = p.view?.harnesses.find(h => h.agent === p.picked);
  const error = p.error && <FailBanner message={p.error} dismiss={p.dismissError} />;
  if (p.view === null) return error || <div className="cc-err">Could not read the coding agents.</div>;
  if (p.inSettings) return <>{error}{hv ? <><Summary hv={hv} p={p} /><Runtime hv={hv} p={p} /></> : <EmptyPanel title={`${p.machineName} does not report ${harnessName(p.picked ?? "")}`} />}</>;
  return <>{error}<div className="hv"><AgentList view={p.view} p={p} />{hv ? <div>{p.shared && <SharedSettings value={p.shared} p={p} />}<Runtime key={`${p.machine}:${hv.agent}`} hv={hv} p={p} /></div> : <div className="hv-d" />}</div></>;
}

function Toolbar({ p }: { p: ProviderProps }) {
  return <div className="hv-bar"><MachinePick p={p} crowded selectLabel="Machine runtime settings" /><span className="sp" /><Button look="quiet" title="Check the versions again" disabled={p.loading || !p.machine} onClick={p.refresh}>{p.loading ? "Checking…" : p.checked}</Button><Button disabled={!!p.updateCount || !p.canUpdate} title={!p.canUpdate && !p.updateCount ? "Everything is up to date" : undefined} onClick={p.updateAll}>{p.updateCount ? `Updating ${p.updateCount}…` : "Update all"}</Button></div>;
}

function SharedSettings({ value, p }: { value: SharedProviderSettings; p: ProviderProps }) {
  const compact = value.compact, directCli = DIRECT_CLIS[value.agent] ?? null;
  useEffect(() => { if (compact && !compact.loaded) p.loadCompact(value.agent); }, [value.agent, compact?.loaded, p.loadCompact]);
  return <section className="hv-scope hv-d"><h3>All machines</h3><div className="hv-note">These settings apply on every machine.</div>
    {compact && <><div className="hv-f"><span><b>Auto-compact</b><small>Applies to sessions on every machine.</small></span><span className="fl-row">
      <Switch label={`Auto-compact for ${harnessName(value.agent)}`} on={compact.draft.compactOn} disabled={!compact.loaded || compact.busy} flip={() => p.changeCompact(value.agent, { compactOn: !compact.draft.compactOn })} />
      {compactKind(value.agent) === "percent" && <input className="hv-in" value={compact.draft.compactPercent} placeholder="80% or 350000" aria-label="Auto-compact threshold" {...NO_AUTO} disabled={!compact.loaded || compact.busy} onChange={event => p.changeCompact(value.agent, { compactPercent: event.currentTarget.value })} />}
    </span></div><div className="hv-acts"><Button look="primary" disabled={!compact.canSave} onClick={() => p.saveCompact(value.agent)}>{compact.busy ? "Saving…" : "Save auto-compact"}</Button><Button look="quiet" hidden={!compact.edited} disabled={compact.busy} onClick={() => p.discardCompact(value.agent)}>Discard</Button><span className="derr">{compact.error}</span></div></>}
    {directCli && <DriverField key={value.agent} agent={value.agent} cli={directCli} invoke={p.invoke} />}
    {(value.agent === "flsh" || value.agent === "pi") && <FlshView panel={p.flsh} agent={value.agent} />}
    {value.machineName && <div className="hv-note">Model choices and reported limits come from {value.machineName}.</div>}
    {p.machine && <ModelContextView panel={p.contexts} machine={p.machine} agent={value.agent} />}
  </section>;
}
/// How the nodes drive a harness; only Claude Code and Codex offer Direct,
/// which runs this CLI without the npm adapter.
type Driver = "acp" | "direct";
const DIRECT_CLIS: Partial<Record<string, string>> = { claude: "claude", codex: "codex" };
const DRIVERS: readonly { id: Driver; label: string }[] = [{ id: "acp", label: "ACP adapter" }, { id: "direct", label: "Direct" }];

function DriverField({ agent, cli, invoke }: { agent: string; cli: string; invoke: typeof Invoke }) {
  const [driver, setDriver] = useState<Driver | null>(null);
  const [error, setError] = useState("");
  useEffect(() => {
    let live = true;
    invoke<string>("harness_driver", { agent }).then(saved => { if (live) setDriver(saved === "direct" ? "direct" : "acp"); }, e => { if (live) setError(String(e)); });
    return () => { live = false; };
  }, [agent, invoke]);
  const pick = (next: Driver) => {
    if (next === driver) return;
    const was = driver;
    setDriver(next);
    setError("");
    invoke("harness_driver_save", { agent, driver: next }).catch(e => { setDriver(was); setError(String(e)); });
  };
  return <div className="hv-f"><span><b>Harness</b><small>Direct runs the {cli} CLI without the npm adapter. New sessions use it.</small></span><span className="fl-row">
    <span className="dchips" role="group" aria-label="Harness">{DRIVERS.map(option => <ChipToggle key={option.id} label={option.label} selected={driver === option.id} onToggle={() => pick(option.id)} />)}</span>
    <span className="derr">{error}</span>
  </span></div>;
}

function HarnessSwitch({ hv, p, on }: { hv: HarnessView; p: ProviderProps; on: boolean }) {
  return <Switch label={`${harnessName(hv.agent)} on ${p.machineName}`} on={on} disabled={p.saving} contained flip={() => p.toggle(hv, !on)} />;
}
function Name({ name, version, latest, status, bad }: { name: string; version: string; latest: string | null; status: string; bad: boolean }) {
  return <span className="hv-name"><span className="hv-nm"><b>{name}</b>{version && <span className="ver">{version}</span>}{latest && <span className="hv-up" title={`v${latest} is out`}>update</span>}</span><small className={bad ? "bad" : ""} title={status}>{status}</small></span>;
}
function AgentList({ view, p }: { view: HarnessesView; p: ProviderProps }) {
  return <div className="hv-list">{listRows(view).map(r => { const hv = view.harnesses.find(h => h.agent === r.agent); if (!hv) return null; return <div key={r.agent} className={`hv-it${r.on || !r.switchable ? "" : " off"}`} role="button" tabIndex={0} aria-current={r.agent === p.picked} onClick={() => p.pick(r.agent)} onKeyDown={onRowKey(() => p.pick(r.agent))}><HarnessIcon agent={r.agent} /><Name name={r.name} version={r.version ? `v${r.version}` : ""} latest={r.update ? hv.latest : null} status={r.status} bad={r.bad} />{r.switchable ? <HarnessSwitch hv={hv} p={p} on={r.on} /> : <span />}</div>; })}</div>;
}
function Summary({ hv, p }: { hv: HarnessView; p: ProviderProps }) {
  const s = p.adapter ? { text: adapterLine(p.adapter), bad: p.adapter.failed !== null } : summaryLine(hv);
  const on = hv.installed && hv.settings.enabled;
  return <div className={`hv-sum${hv.installed && !on ? " off" : ""}`}><HarnessIcon agent={hv.agent} /><Name name={harnessName(hv.agent)} version={hv.installed && hv.version ? `v${hv.version}` : ""} latest={hasUpdate(hv) ? hv.latest : null} status={s.text} bad={s.bad} />{hv.installed ? <><span className="on">{on ? "On" : "Off"} for {p.machineName}</span><HarnessSwitch hv={hv} p={p} on={on} /></> : <><span /><span /></>}</div>;
}
function Field({ label, help, names, children }: { label: string; help: string; names?: readonly string[]; children: ReactNode }) {
  return <div className="hv-f"><span><b>{label}</b>{!!names?.length && <span className="hv-diff" title={`Not the same on ${names.join(", ")}`}>{differsLabel(names)}</span>}<small>{help}</small></span>{children}</div>;
}
function Runtime({ hv, p }: { hv: HarnessView; p: ProviderProps }) {
  const draft = p.draft, name = harnessName(hv.agent), pending = p.adapter, busy = p.updating;
  const field = (label: SharedField) => p.others && p.machine ? differsOn(p.others, p.machine, label) : [];
  const status = hv.sign_in;
  const direct = DIRECT_CLIS[hv.agent];
  const adapterOnly = direct ? ` ACP adapter only: Direct runs the ${direct} CLI found here.` : "";
  return <div className="hv-d">
    {!p.inSettings && <><h3><HarnessIcon agent={hv.agent} />{name}<span className="ver">{versionLine(hv)}</span></h3><h4>Status</h4></>}
    {!hv.installed ? <>{!p.inSettings && <div className="hv-stat">Not installed on {p.machineName}.</div>}{hv.install ? <div className="hv-acts"><Button look="primary" disabled={busy} onClick={() => p.update(hv)}>{busy ? "Installing…" : `Install on ${p.machineName}`}</Button><CopyButton look="quiet" text={hv.install} label="Copy install command" /></div> : <div className="hv-note">Install it on the machine, then check again.</div>}</> : <>
      {!p.inSettings && (status.state === "signed_in" ? <div className="hv-stat"><span className="ddot" />{`Signed in${status.account ? ` as ${status.account}` : ""}${planOf(status)}`}</div> : status.state === "beep_token" ? <div className="hv-stat"><span className="ddot" />{`Signs in with the token in Agent accounts (…${status.last4 ?? ""})`}</div> : status.state === "signed_out" ? <div className="hv-stat"><span className="ddot" style={{ background: "var(--hot)" }} />Not signed in on {p.machineName}</div> : <div className="hv-stat">{name} does not say whether it is signed in. Run it on {p.machineName} to sign in.</div>)}
      {status.state === "beep_token" && <div className="hv-acts"><Button onClick={p.openAccounts}>Agent accounts</Button></div>}
      {status.state === "signed_out" && (hv.agent === "claude" || hv.agent === "codex" ? <div className="hv-acts"><Button look="primary" onClick={p.openAccounts}>Sign in</Button></div> : <div className="hv-note">Run {hv.agent} on {p.machineName} and sign in there.</div>)}
      {status.state === "unknown" && p.inSettings && <div className="hv-note">{name} does not say whether it is signed in. Run it on {p.machineName} to sign in.</div>}
      {pending ? <>{(!p.inSettings || pending.failed !== null) && <div className={`hv-note${pending.failed === null ? "" : " bad"}`}>{adapterLine(pending)}</div>}{pending.failed !== null && <div className="hv-acts"><Button disabled={busy} onClick={() => p.retryAdapter(hv.agent)}>{busy ? "Trying again…" : "Try again"}</Button></div>}</> : !hv.runnable && <div className="hv-note">Restart beep-node to use it.</div>}
      {hasUpdate(hv) && <div className="hv-acts"><Button disabled={busy || !hv.install} title={!hv.install ? "Beep knows no way to update it here" : undefined} onClick={() => p.update(hv)}>{busy ? "Updating…" : `Update to v${hv.latest}`}</Button></div>}
      {draft && <><h4>Runtime</h4><Field label="Binary path" help={`The command Beep starts for ${name} on ${p.machineName}.${adapterOnly}`} names={field("program")}><TextInput label="Binary path" placeholder={hv.program ?? hv.agent} value={draft.program} change={v => p.setDraft({ program: v })} /></Field><Field label={hv.home_var ?? "Home folder"} help="Its home and config folder." names={field("home")}><TextInput label={hv.home_var ?? "Home folder"} placeholder={hv.home_default ?? ""} value={draft.home} change={v => p.setDraft({ home: v })} /></Field><Field label="Launch arguments" help={`Added to every session it starts.${adapterOnly}`} names={field("args")}><TextInput label="Launch arguments" placeholder="none" value={draft.args} change={v => p.setDraft({ args: v })} /></Field>
      <Field label="Environment" help="Encrypted, never shown again. Put keys in Credentials." names={field("env")}><div className="hv-envs">{[...draft.env, { name: "", value: "", saved: false }].map((v, i) => { const last = i === draft.env.length; const set = (change: Partial<typeof v>) => { const env = [...draft.env]; if (last) env.push({ ...v, ...change }); else env[i] = { ...v, ...change }; p.setDraft({ env }); }; return <div className="hv-env" key={i}><TextInput label="Variable name" placeholder="NAME" value={v.name} change={value => set({ name: value })} /><TextInput type="password" label="Value" placeholder={v.saved ? "saved; type to replace" : "value"} value={v.value} change={value => set({ value })} />{last ? <span /> : <Button look="quiet" small aria-label={`Remove ${v.name}`} onClick={() => p.setDraft({ env: draft.env.filter((_, j) => j !== i) })}>×</Button>}</div>; })}</div></Field>
      <div className="hv-acts hv-save"><Button look="primary" title={`Save for ${p.machineName} only`} disabled={p.saving || !draftChanged(hv, draft) || !!envProblem(draft.env)} onClick={() => p.save(hv)}>{p.saving ? "Saving…" : "Save"}</Button>{p.others && p.others.length > 1 && <><Button title="Save these settings on every machine; offline ones get them when they next connect" disabled={p.saving || !!envProblem(draft.env)} onClick={() => p.saveAll(hv)}>{p.saving ? "Saving…" : "Save to all hosts"}</Button><label className="hv-incl" title="Binary paths usually differ between machines"><Checkbox checked={p.includeProgram} change={p.setIncludeProgram} />Include binary path</label></>}<Button look="quiet" hidden={!draftChanged(hv, draft)} onClick={p.discard}>Discard</Button><span className="hv-ok">{draftChanged(hv, draft) ? "" : p.savedAll}</span><span className="derr">{envProblem(draft.env)}</span></div></>}
    </>}
  </div>;
}
function TextInput({ value, label, placeholder, change, type = "text" }: { value: string; label: string; placeholder: string; change: (value: string) => void; type?: "text" | "password" }) {
  return <input type={type} className="hv-in" value={value} placeholder={placeholder} {...NO_AUTO} aria-label={label} onChange={e => change(e.currentTarget.value)} />;
}

export function ProviderHeader(p: Pick<ProviderProps, "machines" | "machine" | "loading" | "checked" | "updateCount" | "canUpdate" | "pickMachine" | "refresh" | "updateAll">) {
  const machines = p.machines ?? [];
  const crowded = machines.length > 4 || machines.reduce((sum, machine) => sum + machine.name.length, 0) > 48;
  return <><MachinePick p={p} crowded={crowded} selectLabel="Machine" /><Button disabled={!!p.updateCount || !p.canUpdate} title={!p.canUpdate && !p.updateCount ? "Everything is up to date" : undefined} onClick={p.updateAll}>{p.updateCount ? `Updating ${p.updateCount}…` : "Update all"}</Button><Button look="quiet" disabled={p.loading || !p.machine} title="Check the versions again" onClick={p.refresh}>{p.loading ? "Checking…" : p.checked}</Button></>;
}

/// The machine picker: a row of buttons, or a dropdown where they do not fit.
function MachinePick({ p, crowded, selectLabel }: { p: Pick<ProviderProps, "machines" | "machine" | "pickMachine">; crowded: boolean; selectLabel: string }) {
  const machines = p.machines ?? [];
  const label = (machine: { name: string; online: boolean }) => machine.online ? machine.name : `${machine.name} (offline)`;
  return <span className={`hv-pick${crowded ? " crowded" : ""}`}>
    <span className="hv-seg" role="group" aria-label="Machine">{machines.map(machine => <button type="button" key={machine.id} title={label(machine)} aria-pressed={machine.id === p.machine} disabled={!machine.online} onClick={() => p.pickMachine(machine.id)}>{machine.name}</button>)}</span>
    <Picker label={selectLabel} className="hv-sel" options={machines.map(machine => ({ id: machine.id, label: machine.name, disabled: !machine.online, tag: machine.online ? undefined : "Offline" }))} value={p.machine} onChange={p.pickMachine} />
  </span>;
}
