import { useLayoutEffect, useRef, type ReactNode } from "react";
import { ModelContextView } from "./model-context-view.tsx";
import type { ModelContextPanel } from "./model-context.ts";
import { FlshView } from "./flsh-view.tsx";
import type { FlshPanel } from "./flsh.ts";
import { copy } from "../ui/parts.ts";
import { splitFailure } from "../ui/failbanner.ts";
import { harnessInner, harnessMark } from "../ui/harnessicon.ts";
import { adapterLine, type AdapterInstall } from "../machines/adapters-state.ts";
import { differsLabel, differsOn, draftChanged, envProblem, harnessName, hasUpdate, listRows, planOf, summaryLine, versionLine, type HarnessDraft, type HarnessesView, type HarnessView, type MachineSettings, type SharedField } from "./providers-state.ts";

interface ProviderProps {
  machines: { id: string; name: string; online: boolean }[] | null | undefined;
  machine: string | null; machineName: string; view: HarnessesView | null | false | undefined; picked: string | null;
  draft: HarnessDraft | null; others: MachineSettings[] | false | undefined; saving: boolean; updating: boolean;
  adapter: AdapterInstall | null | undefined; error: string; inSettings: boolean; savedAll: string; includeProgram: boolean;
  shared: { before: Node; after: Node; agent: string } | null;
  flsh: FlshPanel; contexts: ModelContextPanel;
  loading: boolean; checked: string; updateCount: number; canUpdate: boolean;
  pick: (agent: string) => void; toggle: (hv: HarnessView, on: boolean) => void; update: (hv: HarnessView) => void;
  retryAdapter: (agent: string) => void; openAccounts: () => void; setDraft: (value: Partial<HarnessDraft>) => void;
  save: (hv: HarnessView) => void; saveAll: (hv: HarnessView) => void; discard: () => void;
  setIncludeProgram: (value: boolean) => void; dismissError: () => void;
  pickMachine: (id: string) => void; refresh: () => void; updateAll: () => void;
}

export function ProvidersView(p: ProviderProps) {
  return <div className={`cc-view hv-view${p.inSettings ? " in-set" : ""}`}>
    {p.inSettings ? <>{p.shared && <SharedSettings value={p.shared} flsh={p.flsh} contexts={p.contexts} machine={p.machine} />}<section className="hv-scope"><Toolbar p={p} /><Body {...p} /></section></> : <Body {...p} />}
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
  return <>{error}<div className="hv"><AgentList view={p.view} p={p} />{hv ? <div>{p.shared && <SharedSettings value={p.shared} flsh={p.flsh} contexts={p.contexts} machine={p.machine} />}<Runtime key={`${p.machine}:${hv.agent}`} hv={hv} p={p} /></div> : <div className="hv-d" />}</div></>;
}

function Toolbar({ p }: { p: ProviderProps }) {
  const machines = p.machines ?? [];
  return <div className="hv-bar"><span className="hv-pick crowded">
    <span className="hv-seg" role="group" aria-label="Machine">{machines.map(m => <button key={m.id} type="button" title={m.online ? m.name : `${m.name} (offline)`} aria-pressed={m.id === p.machine} disabled={!m.online} onClick={() => p.pickMachine(m.id)}>{m.name}</button>)}</span>
    <select className="hv-sel" value={p.machine ?? ""} aria-label="Machine runtime settings" onChange={e => p.pickMachine(e.currentTarget.value)}>{machines.map(m => <option key={m.id} value={m.id} disabled={!m.online}>{m.online ? m.name : `${m.name} (offline)`}</option>)}</select>
  </span><span className="sp" /><button type="button" className="dbtn quiet" title="Check the versions again" disabled={p.loading || !p.machine} onClick={p.refresh}>{p.loading ? "Checking…" : p.checked}</button><button type="button" className="dbtn" disabled={!!p.updateCount || !p.canUpdate} title={!p.canUpdate && !p.updateCount ? "Everything is up to date" : undefined} onClick={p.updateAll}>{p.updateCount ? `Updating ${p.updateCount}…` : "Update all"}</button></div>;
}

function Empty({ title, sub }: { title: string; sub?: string }) { return <div className="cc-panel cc-empty"><b>{title}</b>{sub && <span>{sub}</span>}</div>; }
function SharedSettings({ value, flsh, contexts, machine }: { value: NonNullable<ProviderProps["shared"]>; flsh: FlshPanel; contexts: ModelContextPanel; machine: string | null }) {
  return <section className="hv-scope hv-d"><h3>All machines</h3><LegacyNode node={value.before} />{value.agent === "flsh" && <FlshView panel={flsh} />}<LegacyNode node={value.after} />{machine && <ModelContextView panel={contexts} machine={machine} agent={value.agent} />}</section>;
}
function LegacyNode({ node }: { node: Node }) {
  const host = useRef<HTMLDivElement>(null);
  useLayoutEffect(() => {
    const target = host.current;
    if (!target) return;
    const active = document.activeElement;
    const input = active instanceof HTMLInputElement && target.contains(active) ? active : null;
    const label = input?.getAttribute("aria-label");
    const start = input?.selectionStart ?? null, end = input?.selectionEnd ?? null;
    target.replaceChildren(node);

    if (label) {
      const next = [...target.querySelectorAll<HTMLInputElement>("input[aria-label]")].find(field => field.getAttribute("aria-label") === label);
      next?.focus({ preventScroll: true });
      if (next && start !== null && end !== null) next.setSelectionRange(start, end);
    }
  }, [node]);
  return <div ref={host} style={{ display: "contents" }} />;
}
function Mark({ agent }: { agent: string }) {
  const mark = harnessMark(agent), safe = /^[a-z0-9-]+$/.test(agent) ? ` ${agent}` : "";
  return <span className={`cc-ag${safe}${mark ? " mark" : ""}`} dangerouslySetInnerHTML={{ __html: harnessInner(agent) }} />;
}
function Switch({ hv, p, on }: { hv: HarnessView; p: ProviderProps; on: boolean }) {
  return <button type="button" className="dswitch" role="switch" aria-checked={on} aria-label={`${harnessName(hv.agent)} on ${p.machineName}`} disabled={p.saving} onClick={e => { e.stopPropagation(); p.toggle(hv, !on); }} onKeyDown={e => e.stopPropagation()} />;
}
function Name({ name, version, latest, status, bad }: { name: string; version: string; latest: string | null; status: string; bad: boolean }) {
  return <span className="hv-name"><span className="hv-nm"><b>{name}</b>{version && <span className="ver">{version}</span>}{latest && <span className="hv-up" title={`v${latest} is out`}>update</span>}</span><small className={bad ? "bad" : ""} title={status}>{status}</small></span>;
}
function AgentList({ view, p }: { view: HarnessesView; p: ProviderProps }) {
  return <div className="hv-list">{listRows(view).map(r => { const hv = view.harnesses.find(h => h.agent === r.agent); if (!hv) return null; return <div key={r.agent} className={`hv-it${r.on || !r.switchable ? "" : " off"}`} role="button" tabIndex={0} aria-current={r.agent === p.picked} onClick={() => p.pick(r.agent)} onKeyDown={e => { if (e.target === e.currentTarget && (e.key === "Enter" || e.key === " ")) { e.preventDefault(); p.pick(r.agent); } }}><Mark agent={r.agent} /><Name name={r.name} version={r.version ? `v${r.version}` : ""} latest={r.update ? hv.latest : null} status={r.status} bad={r.bad} />{r.switchable ? <Switch hv={hv} p={p} on={r.on} /> : <span />}</div>; })}</div>;
}
function Summary({ hv, p }: { hv: HarnessView; p: ProviderProps }) {
  const s = p.adapter ? { text: adapterLine(p.adapter), bad: p.adapter.failed !== null } : summaryLine(hv);
  const on = hv.installed && hv.settings.enabled;
  return <div className={`hv-sum${hv.installed && !on ? " off" : ""}`}><Mark agent={hv.agent} /><Name name={harnessName(hv.agent)} version={hv.installed && hv.version ? `v${hv.version}` : ""} latest={hasUpdate(hv) ? hv.latest : null} status={s.text} bad={s.bad} />{hv.installed ? <><span className="on">{on ? "On" : "Off"} for {p.machineName}</span><Switch hv={hv} p={p} on={on} /></> : <><span /><span /></>}</div>;
}
function Field({ label, help, names, children }: { label: string; help: string; names?: readonly string[]; children: ReactNode }) {
  return <div className="hv-f"><span><b>{label}</b>{!!names?.length && <span className="hv-diff" title={`Not the same on ${names.join(", ")}`}>{differsLabel(names)}</span>}<small>{help}</small></span>{children}</div>;
}
function Runtime({ hv, p }: { hv: HarnessView; p: ProviderProps }) {
  const draft = p.draft, name = harnessName(hv.agent), pending = p.adapter, busy = p.updating;
  const field = (label: SharedField) => p.others && p.machine ? differsOn(p.others, p.machine, label) : [];
  const status = hv.sign_in;
  return <div className="hv-d">
    {!p.inSettings && <><h3><Mark agent={hv.agent} />{name}<span className="ver">{versionLine(hv)}</span></h3><h4>Status</h4></>}
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
