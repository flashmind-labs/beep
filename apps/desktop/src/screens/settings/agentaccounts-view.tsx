import { Button, CopyButton, NO_AUTO, SkeletonRows, Spinner } from "../../ui/kit.tsx";
import { useLayoutEffect, useRef, useState } from "react";
import type { ReactNode } from "react";
import { HarnessIcon, Icon } from "../../ui/icons.tsx";
import { onPhone } from "../../ui/platform.ts";
import type { AccountAgent, AgentAccounts, AgentAccountsState } from "../../stores/settings/agentaccounts.ts";

type Machine = AgentAccounts["machines"][number];
type DotTone = "ok" | "warn" | "off";
type Actions = { add: (agent: AccountAgent | null) => void; addSave: (name: string, secret: string) => void; addSign: (name: string) => void; removeNamed: (agent: AccountAgent, name: string) => void; edit: (agent: "claude" | "codex") => void; cancel: () => void; save: (secret: string, clear: () => void) => void; revoke: (agent: "claude" | "codex") => void; claudeSign: () => void; claudeCancel: () => void; machineSign: (tool: "claude" | "agy", machine: { id: string; name: string; online: boolean }) => void; codexSign: (id: string) => void; codexCancel: (id: string) => void; open: (url: string) => void };
function Row({ name, tone, children }: { name: string; tone: DotTone; children: ReactNode }) { return <div className="dline dsm dpy"><span className={`ddot ${tone}`} /><span>{name}</span><span className="dacts dmut">{children}</span></div>; }
function Form({ state, agent, actions }: { state: AgentAccountsState; agent: "claude" | "codex"; actions: Actions }) {
  const input = useRef<HTMLInputElement>(null), clear = () => { if (input.current) input.current.value = ""; };
  useLayoutEffect(() => { input.current?.focus(); }, []);
  return <div className="dgrid8 dmt6"><div className="dline"><input ref={input} id="agSecret" type="password" {...NO_AUTO} placeholder={agent === "claude" ? "Token from claude setup-token" : "OpenAI API key"} aria-label={agent === "claude" ? "Claude Code token" : "Codex API key"} className="dsecret" onKeyDown={e => { if (e.key === "Enter") { e.preventDefault(); actions.save(e.currentTarget.value, clear); } }} /><Button look="primary" id="agSave" disabled={!!state.busy} onClick={() => actions.save(input.current?.value ?? "", clear)}>{state.busy === agent ? "Saving…" : "Save"}</Button><Button look="quiet" id="agCancel" onClick={actions.cancel}>Cancel</Button></div><span className="dsub">{agent === "claude" ? <>Run <code>claude setup-token</code> in Terminal and paste the token it prints.</> : "An OpenAI API key. Codex uses it on every machine instead of signing in."}</span><span className="derr" id="agErr">{state.error || ""}</span></div>;
}
/// The agent's named accounts, inside its card: one row each and the add form.
function NamedAccounts({ agent, state, data, ago, actions }: { agent: AccountAgent; state: AgentAccountsState; data: AgentAccounts; ago: (ms: number) => string; actions: Actions }) {
  const named = data.named.filter(account => account.agent === agent), add = state.add?.agent === agent ? state.add : null;
  const addButton = !add && <span className="dacts"><Button look="quiet" small onClick={() => actions.add(agent)}><Icon name="plus" size={12} />Add account</Button></span>;
  return <div className="daccounts">
    <div className={named.length ? "dline" : "dline daccounts-empty"}>{named.length > 0 && <span className="dfine">Other accounts</span>}{addButton}</div>
    {named.map(account => <div className="dline dsm dpy dnamed" key={account.name}><span>{account.name}</span><span className="dacts dmut"><code>…{account.token.last4}</code> · added {ago(account.token.created_at)}<Button look="quiet" danger small disabled={state.busy === `remove:${agent}/${account.name}`} onClick={() => actions.removeNamed(agent, account.name)}>Remove</Button></span></div>)}
    {state.namedError?.agent === agent && <div className="derr dmt6">{state.namedError.text}</div>}
    {add && <AddAccount state={state} agent={agent} error={add.error} actions={actions} />}
  </div>;
}
/// Adds a named account: Claude Code signs in through the browser, with a
/// pasted token as the fallback; Codex takes a ChatGPT personal access token.
function AddAccount({ state, agent, error, actions }: { state: AgentAccountsState; agent: AccountAgent; error: string; actions: Actions }) {
  const name = useRef<HTMLInputElement>(null), secret = useRef<HTMLInputElement>(null), signs = agent === "claude" && !onPhone();
  const [paste, setPaste] = useState(!signs);
  const save = () => actions.addSave(name.current?.value ?? "", secret.current?.value ?? "");
  const cancel = () => { if (state.claudeFlow) actions.claudeCancel(); actions.add(null); };
  const busy = !!state.busy || !!state.claudeFlow;
  useLayoutEffect(() => { name.current?.focus(); }, []);
  return <div className="dgrid8 dmt6">
    <div className="dline"><input ref={name} id="agName" type="text" {...NO_AUTO} placeholder="Name, such as Work" aria-label="Account name" className="dname" disabled={state.claudeFlow} onKeyDown={e => { if (e.key === "Enter" && !paste) { e.preventDefault(); actions.addSign(e.currentTarget.value); } }} />
      {paste && <input ref={secret} id="agNamedSecret" type="password" {...NO_AUTO} placeholder={agent === "claude" ? "Token from claude setup-token" : "ChatGPT personal access token"} aria-label={agent === "claude" ? "Claude Code token" : "Codex access token"} className="dsecret" disabled={state.claudeFlow} onKeyDown={e => { if (e.key === "Enter") { e.preventDefault(); save(); } }} />}
      {paste ? <Button look="primary" id="agNamedSave" disabled={busy} onClick={save}>{state.busy === "add" ? "Saving…" : "Save"}</Button>
        : <Button look="primary" id="agNamedSign" disabled={busy} onClick={() => actions.addSign(name.current?.value ?? "")}>Sign in</Button>}
      {signs && !paste && <Button look="quiet" id="agNamedPaste" disabled={busy} onClick={() => setPaste(true)}>Paste a token instead</Button>}
      <Button look="quiet" onClick={cancel}>Cancel</Button></div>
    {state.claudeFlow ? <div className="dfine dline"><Spinner />Approve in your browser, signed in to the account this name is for.</div>
      : <span className="dsub">{!paste ? "Signs in through your browser. Use the account this name is for." : agent === "claude" ? <>Run <code>claude setup-token</code> while signed in to that account and paste what it prints.</> : "A ChatGPT personal access token for that subscription."}</span>}
    <span className="derr" id="agNamedErr">{error}</span></div>;
}
function ownLogin(data: AgentAccounts, m: Machine) { const login = data.claude_logins?.get(m.id); return !login ? "not checked" : !login.signed_in ? "not signed in" : login.account ? `signed in as ${login.account}` : "signed in"; }
function usedOn(m: Machine) { if (!m.online) return "offline · gets it when back"; return !m.claude_sessions ? "idle" : m.claude_sessions === 1 ? "1 session now" : `${m.claude_sessions} sessions now`; }
function timeLeft(expires: number, now: number) { const s = Math.floor((expires - now) / 1000); return s > 0 ? `${Math.floor(s / 60)}:${String(s % 60).padStart(2, "0")} left` : "expired"; }
export function AgentAccountsView({ state, ago, now, actions }: { state: AgentAccountsState; ago: (ms: number) => string; now: number; actions: Actions }) {
  const data = state.data;
  if (data === null) return <div className="dmuted">Could not load your agent accounts.</div>;
  if (!data) return <SkeletonRows widths={[78, 56]} />;
  const phone = onPhone(), flow = state.claudeFlow && state.add?.agent !== "claude", claude = data.claude, claudeMachines = data.machines.filter(m => m.claude), key = data.codex_key, codexMachines = data.machines.filter(m => m.codex);
  return <><div className="dlead">How your coding agents sign in across your machines.</div><div className="dbox">
    <div className="dline"><HarnessIcon agent="claude" /><span className="dstack"><b className="dtitle">Claude Code</b>{!flow && claude && !claude.unreadable && <span className="dsub">signed in {ago(claude.created_at)} · <code>…{claude.last4}</code></span>}</span>
      {flow ? <span className="dpush"><Button look="quiet" small onClick={e => { e.currentTarget.disabled = true; actions.claudeCancel(); }}>Cancel</Button></span> : claude && !claude.unreadable ? <span className="dacts">{!phone && <Button disabled={state.claudeFlow} onClick={actions.claudeSign}>Replace</Button>}<Button danger disabled={state.busy === "revoke:claude"} onClick={() => actions.revoke("claude")}>Revoke</Button></span> : <span className="dacts">{!phone && <Button look={claudeMachines.length ? "secondary" : "primary"} disabled={state.claudeFlow} onClick={actions.claudeSign}>{claudeMachines.length ? "Sign in for all" : "Sign in"}</Button>}{state.edit !== "claude" && <Button look={phone ? "secondary" : "quiet"} onClick={() => actions.edit("claude")}>Paste a token</Button>}</span>}</div>
    {flow ? <div className="dfine dline dmt6"><Spinner />Approve in your browser. Beep saves the token once you do.</div> : <>
      {!(claude && !claude.unreadable) && <>{claudeMachines.length > 0 && <div className="dmt6">{claudeMachines.map(m => { const signed = !!data.claude_logins?.get(m.id)?.signed_in; return <Row key={m.id} name={m.name} tone={!m.online ? "off" : signed ? "ok" : "warn"}>{ownLogin(data, m)}{m.online && !signed && <Button small onClick={() => actions.machineSign("claude", m)}>Sign in</Button>}</Row>; })}</div>}<div className="dfine dmt6">{claude?.unreadable ? "Beep can no longer read the saved token. Sign in again." : phone ? "Paste a token from claude setup-token to give every machine one token. Beep keeps it encrypted." : "Sign in for all to give every machine one token. Beep keeps it encrypted and never writes it to their disks."}</div></>}
      {state.edit === "claude" && <Form state={state} agent="claude" actions={actions} />}{state.claudeError && <div className="derr dmt6">{state.claudeError}</div>}
      {claude && !claude.unreadable && <><div className="dfine dmt6">Used on</div>{claudeMachines.length ? claudeMachines.map(m => <Row key={m.id} name={m.name} tone={m.online ? "ok" : "off"}>{usedOn(m)}</Row>) : <div className="dfine dpy">No machine has Claude Code yet.</div>}{state.edit !== "claude" && <Button look="quiet" small onClick={() => actions.edit("claude")}>Paste a token instead</Button>}</>}
    </>}
    <NamedAccounts agent="claude" state={state} data={data} ago={ago} actions={actions} />
  </div><div className="dbox"><div className="dline"><HarnessIcon agent="codex" /><span className="dstack"><b className="dtitle">Codex</b>{key && <span className="dsub">{!key.unreadable ? <>API key <code>…{key.last4}</code> on every machine · added {ago(key.created_at)}</> : "Beep can no longer read the saved API key"}</span>}</span><span className="dacts">{state.edit !== "codex" && (key ? <><Button small onClick={() => actions.edit("codex")}>Replace key</Button><Button look="quiet" danger small disabled={state.busy === "revoke:codex"} onClick={() => actions.revoke("codex")}>Remove key</Button></> : <Button onClick={() => actions.edit("codex")}>Use an API key</Button>)}</span></div>
    {state.edit === "codex" && <Form state={state} agent="codex" actions={actions} />}
    <div className="dmt6">{codexMachines.length ? codexMachines.map(m => { const flow = state.codex?.[m.id]; if (flow?.code) return <div className="dflow" key={m.id}><span>{m.name}: open <b>{flow.url.replace(/^https:\/\//, "")}</b> and enter</span><span className="dflowcode">{flow.code}</span><span className="dmuted"><Spinner />Waiting for you to approve · {timeLeft(flow.expires_at, now)}<span className="dacts"><CopyButton small text={flow.code} label={flow.copied ? "Copied" : "Copy code"} /><Button look="quiet" small onClick={() => actions.open(flow.url)}>Open page</Button><Button look="quiet" small onClick={e => { e.currentTarget.disabled = true; actions.codexCancel(m.id); }}>Cancel</Button></span></span></div>; const signed = !!m.codex_signed_in || !!(key && !key.unreadable); return <div key={m.id}><Row name={m.name} tone={!m.online ? "off" : signed ? "ok" : "warn"}>{!m.online ? "offline" : flow?.starting ? <><Spinner />starting sign-in</> : m.codex_signed_in ? "signed in" : <>{m.codex_signed_in !== false && <span>not checked</span>}<Button small onClick={() => actions.codexSign(m.id)}>Sign in</Button></>}</Row>{flow?.error && <div className="derr dind">{flow.error}</div>}</div>; }) : <div className="dfine dpy">No machine has Codex yet.</div>}</div>
    <NamedAccounts agent="codex" state={state} data={data} ago={ago} actions={actions} />
  </div>{!!data.agy_machines?.length && <div className="dbox"><div className="dline"><HarnessIcon agent="agy" /><span className="dstack"><b className="dtitle">Antigravity</b></span></div>
    <div className="dmt6">{data.agy_machines.map(m => <Row key={m.id} name={m.name} tone={!m.online ? "off" : m.signed_in ? "ok" : "warn"}>{m.signed_in ? "signed in" : "not signed in"}{m.online && !m.signed_in && <Button small onClick={() => actions.machineSign("agy", m)}>Sign in</Button>}</Row>)}</div>
  </div>}</>;
}
export type AgentAccountActions = Actions;
