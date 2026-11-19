import { Button, ChipToggle, CopyButton, SkeletonRows, Spinner } from "../ui/kit.tsx";
import { useLayoutEffect, useRef } from "react";
import type { ReactNode } from "react";
import { HarnessIcon } from "../ui/icons.tsx";
import { onPhone } from "../ui/platform.ts";
import type { AccountAgent, AgentAccounts, AgentAccountsState } from "./agentaccounts.ts";

type Machine = AgentAccounts["machines"][number];
type Actions = { add: (agent: AccountAgent | null) => void; addSave: (name: string, secret: string) => void; addSign: (name: string) => void; removeNamed: (agent: AccountAgent, name: string) => void; edit: (agent: "claude" | "codex") => void; cancel: () => void; save: (secret: string, clear: () => void) => void; revoke: (agent: "claude" | "codex") => void; claudeSign: () => void; claudeCancel: () => void; claudeMachineSign: (id: string) => void; codexSign: (id: string) => void; codexCancel: (id: string) => void; open: (url: string) => void };
function Row({ name, offline, children }: { name: string; offline: boolean; children: ReactNode }) { return <div className="dline dsm dpy"><span className={`ddot${offline ? " off" : ""}`} /><span>{name}</span><span className="dacts dmut">{children}</span></div>; }
function Form({ state, agent, actions }: { state: AgentAccountsState; agent: "claude" | "codex"; actions: Actions }) {
  const input = useRef<HTMLInputElement>(null), clear = () => { if (input.current) input.current.value = ""; };
  useLayoutEffect(() => { input.current?.focus(); }, []);
  return <div className="dgrid8 dmt6"><div className="dline"><input ref={input} id="agSecret" type="password" autoComplete="off" spellCheck={false} placeholder={agent === "claude" ? "sk-ant-oat01-…" : "sk-…"} aria-label={agent === "claude" ? "Claude Code token" : "Codex API key"} className="dsecret" onKeyDown={e => { if (e.key === "Enter") { e.preventDefault(); actions.save(e.currentTarget.value, clear); } }} /><Button look="primary" id="agSave" disabled={!!state.busy} onClick={() => actions.save(input.current?.value ?? "", clear)}>{state.busy === agent ? "Saving…" : "Save"}</Button><Button look="quiet" id="agCancel" onClick={actions.cancel}>Cancel</Button></div><span className="dsub">{agent === "claude" ? <>Run <code>claude setup-token</code> in Terminal and paste the token it prints.</> : "An OpenAI API key. Codex uses it on every machine instead of signing in."}</span><span className="derr" id="agErr">{state.error || ""}</span></div>;
}
const AGENT_NAMES: Record<AccountAgent, string> = { claude: "Claude Code", codex: "Codex" };
function AddAccount({ state, agent, error, actions }: { state: AgentAccountsState; agent: AccountAgent; error: string; actions: Actions }) {
  const name = useRef<HTMLInputElement>(null), secret = useRef<HTMLInputElement>(null), phone = onPhone();
  const save = () => actions.addSave(name.current?.value ?? "", secret.current?.value ?? "");
  useLayoutEffect(() => { name.current?.focus(); }, []);
  return <div className="dgrid8 dmt6">
    <div className="dline">{(["claude", "codex"] as const).map(a => <ChipToggle key={a} label={AGENT_NAMES[a]} selected={a === agent} onToggle={() => actions.add(a)} />)}</div>
    <div className="dline"><input ref={name} id="agName" type="text" autoComplete="off" spellCheck={false} placeholder="Work" aria-label="Account name" className="dsecret" /></div>
    <div className="dline"><input ref={secret} id="agNamedSecret" type="password" autoComplete="off" spellCheck={false} placeholder={agent === "claude" ? "sk-ant-oat01-…" : "Personal access token"} aria-label={agent === "claude" ? "Claude Code token" : "Codex access token"} className="dsecret" onKeyDown={e => { if (e.key === "Enter") { e.preventDefault(); save(); } }} />
      <Button look="primary" id="agNamedSave" disabled={!!state.busy || state.claudeFlow} onClick={save}>{state.busy === "add" ? "Saving…" : "Save"}</Button>
      {agent === "claude" && !phone && <Button disabled={!!state.busy || state.claudeFlow} onClick={() => actions.addSign(name.current?.value ?? "")}>Sign in instead</Button>}
      <Button look="quiet" onClick={() => actions.add(null)}>Cancel</Button></div>
    {state.claudeFlow ? <div className="dfine dline"><Spinner />Approve in your browser, signed in to the account this name is for.</div> : <span className="dsub">{agent === "claude" ? <>Paste the token <code>claude setup-token</code> prints while signed in to this account, or sign in here.</> : "A ChatGPT personal access token for that subscription."}</span>}
    <span className="derr" id="agNamedErr">{error}</span></div>;
}
function NamedAccounts({ state, data, ago, actions }: { state: AgentAccountsState; data: AgentAccounts; ago: (ms: number) => string; actions: Actions }) {
  const add = state.add;
  return <div className="dbox"><div className="dline"><span className="dstack"><b className="dtitle">Named accounts</b><span className="dsub">Work, personal and other accounts to pick per project or session. Without one, a session uses the account above.</span></span>{!add && <span className="dacts"><Button onClick={() => actions.add("claude")}>Add account</Button></span>}</div>
    {data.named.length > 0 && <div className="dmt6">{data.named.map(account => <div className="dline dsm dpy" key={`${account.agent}/${account.name}`}><HarnessIcon agent={account.agent} /><span>{account.name}</span><span className="dacts dmut"><code>…{account.token.last4}</code> · added {ago(account.token.created_at)}<Button look="quiet" danger small disabled={state.busy === `remove:${account.agent}/${account.name}`} onClick={() => actions.removeNamed(account.agent, account.name)}>Remove</Button></span></div>)}</div>}
    {state.namedError && <div className="derr dmt6">{state.namedError}</div>}
    {add && <AddAccount state={state} agent={add.agent} error={add.error} actions={actions} />}
  </div>;
}
function ownLogin(data: AgentAccounts, m: Machine) { const login = data.claude_logins?.get(m.id); return !login ? "not checked" : !login.signed_in ? "not signed in" : login.account ? `signed in as ${login.account}` : "signed in"; }
function usedOn(m: Machine) { if (!m.online) return "offline · gets it when back"; return !m.claude_sessions ? "idle" : m.claude_sessions === 1 ? "1 session now" : `${m.claude_sessions} sessions now`; }
function timeLeft(expires: number, now: number) { const s = Math.floor((expires - now) / 1000); return s > 0 ? `${Math.floor(s / 60)}:${String(s % 60).padStart(2, "0")} left` : "expired"; }
export function AgentAccountsView({ state, ago, now, actions }: { state: AgentAccountsState; ago: (ms: number) => string; now: number; actions: Actions }) {
  const data = state.data;
  if (data === null) return <div className="dmuted">Could not load your agent accounts.</div>;
  if (!data) return <SkeletonRows widths={[78, 56]} />;
  const phone = onPhone(), claude = data.claude, claudeMachines = data.machines.filter(m => m.claude), key = data.codex_key, codexMachines = data.machines.filter(m => m.codex);
  return <><div className="dlead">How your coding agents sign in on your machines.</div><div className="dbox">
    <div className="dline"><HarnessIcon agent="claude" /><span className="dstack"><b className="dtitle">Claude Code</b>{!state.claudeFlow && (claude && !claude.unreadable ? <span className="dsub">signed in {ago(claude.created_at)} · <code>…{claude.last4}</code></span> : claudeMachines.length > 0 && <span className="dsub">signs in on each machine</span>)}</span>
      {state.claudeFlow ? <span className="dpush"><Button look="quiet" small onClick={e => { e.currentTarget.disabled = true; actions.claudeCancel(); }}>Cancel</Button></span> : claude && !claude.unreadable ? <span className="dacts">{!phone && <Button onClick={actions.claudeSign}>Replace</Button>}<Button danger disabled={state.busy === "revoke:claude"} onClick={() => actions.revoke("claude")}>Revoke</Button></span> : <span className="dacts">{!phone && <Button look={claudeMachines.length ? "secondary" : "primary"} onClick={actions.claudeSign}>{claudeMachines.length ? "Sign in for all" : "Sign in"}</Button>}{state.edit !== "claude" && <Button look={phone ? "secondary" : "quiet"} onClick={() => actions.edit("claude")}>Paste a token</Button>}</span>}</div>
    {state.claudeFlow ? <div className="dfine dline dmt6"><Spinner />Approve in your browser. Beep saves the token once you do.</div> : <>
      {!(claude && !claude.unreadable) && <>{claudeMachines.length > 0 && <div className="dmt6">{claudeMachines.map(m => <Row key={m.id} name={m.name} offline={!m.online}>{ownLogin(data, m)}{m.online && !data.claude_logins?.get(m.id)?.signed_in && <Button small onClick={() => actions.claudeMachineSign(m.id)}>Sign in</Button>}</Row>)}</div>}<div className="dfine dmt6">{claude?.unreadable ? "Beep can no longer read the saved token. Sign in again." : phone ? "Paste a token from claude setup-token to give every machine one token. Beep" : "Sign in for all to give every machine one token. Beep keeps it encrypted and never writes it to their disks."}</div></>}
      {state.edit === "claude" && <Form state={state} agent="claude" actions={actions} />}{state.claudeError && <div className="derr dmt6">{state.claudeError}</div>}
      {claude && !claude.unreadable && <><div className="dfine dmt6">Used on</div>{claudeMachines.length ? claudeMachines.map(m => <Row key={m.id} name={m.name} offline={!m.online}>{usedOn(m)}</Row>) : <div className="dfine dpy">No machine has Claude Code yet.</div>}{state.edit !== "claude" && <Button look="quiet" small onClick={() => actions.edit("claude")}>Paste a token instead</Button>}</>}
    </>}
  </div><div className="dbox"><div className="dline"><HarnessIcon agent="codex" /><span className="dstack"><b className="dtitle">Codex</b><span className="dsub">{key && !key.unreadable ? <>API key <code>…{key.last4}</code> on every machine · added {ago(key.created_at)}</> : key ? "Beep can no longer read the saved API key" : "signs in on each machine"}</span></span><span className="dacts">{state.edit !== "codex" && (key ? <><Button small onClick={() => actions.edit("codex")}>Replace key</Button><Button look="quiet" danger small disabled={state.busy === "revoke:codex"} onClick={() => actions.revoke("codex")}>Remove key</Button></> : <Button onClick={() => actions.edit("codex")}>Use an API key</Button>)}</span></div>
    {state.edit === "codex" && <Form state={state} agent="codex" actions={actions} />}
    <div className="dmt6">{codexMachines.length ? codexMachines.map(m => { const flow = state.codex?.[m.id]; if (flow?.code) return <div className="dflow" key={m.id}><span>{m.name}: open <b>{flow.url.replace(/^https:\/\//, "")}</b> and enter</span><span className="dflowcode">{flow.code}</span><span className="dmuted"><Spinner />Waiting for you to approve · {timeLeft(flow.expires_at, now)}<span className="dacts"><CopyButton small text={flow.code} label={flow.copied ? "Copied" : "Copy code"} /><Button look="quiet" small onClick={() => actions.open(flow.url)}>Open page</Button><Button look="quiet" small onClick={e => { e.currentTarget.disabled = true; actions.codexCancel(m.id); }}>Cancel</Button></span></span></div>; return <div key={m.id}><Row name={m.name} offline={!m.online}>{!m.online ? "offline" : flow?.starting ? <><Spinner />starting sign-in</> : m.codex_signed_in ? "signed in" : <>{m.codex_signed_in !== false && <span>not checked</span>}<Button small onClick={() => actions.codexSign(m.id)}>Sign in</Button></>}</Row>{flow?.error && <div className="derr dind">{flow.error}</div>}</div>; }) : <div className="dfine dpy">No machine has Codex yet.</div>}</div>
  </div><NamedAccounts state={state} data={data} ago={ago} actions={actions} /></>;
}
export type AgentAccountActions = Actions;
