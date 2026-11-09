import { useState } from "react";
import type { App, AppAccount, AppPermissions, Level } from "./apps.ts";

interface Props {
  app: App; permissions: AppPermissions | null; error: string; accountsError: string; accounts: AppAccount[] | null;
  connecting: boolean; busy: string | null; removing: string | null; saving: boolean;
  retry(): void; addAccount(): void; cancelSignIn(): void; confirmAccount(id: string): void; cancelAccount(): void; removeAccount(id: string): void; disconnect(): void; setLevel(level: Level): void; toggleAction(tool: string): void;
}
const levels: [Level, string, string][] = [["ask", "Ask first", "Beep shows what it is about to do and waits for you."], ["allow", "Always allow", "Beep sends, changes and deletes without asking."], ["read", "Read only", "Beep can search and read, never change anything."]];
function Logo({ app }: { app: App }) {
  const [failed, setFailed] = useState(false), url = app.slug === "fastmail" ? "fastmail.svg" : app.logo_url;
  return url && !failed ? <img className="dalogo" src={url} alt="" onError={() => setFailed(true)} /> : <span className="dalogo">{app.name.slice(0, 1).toUpperCase()}</span>;
}
export function AppPermissionsView(p: Props) {
  const { app, permissions, accounts } = p;
  return <><div className="dhead"><Logo key={app.slug} app={app} /><span className="tt"><b>{app.name}</b><small>What Beep may do here</small></span></div><div className="dlist dapps" id="plist">
    {p.error ? <div className="derr dpad">{p.error}</div> : !permissions && <div className="dmuted dpad"><span className="dspin" /></div>}
    {p.accountsError && <><div className="dsec">Accounts</div><div className="daction"><span className="derr">{app.name} accounts did not load: {p.accountsError}</span></div></>}
    {(p.error || p.accountsError) && <div className="dpad"><button className="dbtn" onClick={p.retry}>Retry</button></div>}
    {!!accounts?.length && <><div className="dsec">Accounts</div>{accounts.map((account, index) => <div className="daction" key={account.id}><span>{account.address ?? `${app.name} account`}{index === 0 && accounts.length > 1 && <> <small className="dmuted dinline">Default</small></>}</span>{p.busy === `account:${account.id}` ? <span className="dmuted"><span className="dspin" /></span> : p.removing === account.id ? <><span className="dmuted">{accounts.length === 1 ? `Disconnects ${app.name}` : "Beep stops using this account"}</span><button className="dbtn quiet" onClick={p.cancelAccount}>Cancel</button><button className="dbtn warn" onClick={() => p.removeAccount(account.id)}>Remove</button></> : <button className="dbtn quiet warn" onClick={() => p.confirmAccount(account.id)}>Remove</button>}</div>)}
      {p.connecting ? <div className="dpad dline"><span className="dmuted"><span className="dspin" /> Waiting for browser…</span><button className="dbtn quiet" onClick={p.cancelSignIn}>Cancel</button></div> : <div className="dpad"><button className="dbtn" onClick={p.addAccount}>Add another account</button></div>}
      <div className="dmuted dpad dblock">Beep uses the first account unless you name another, like "check my work inbox". To add a different account, sign out of {app.name} in the browser first.</div></>}
    {permissions && <><div className="dsec">When Beep acts in {app.name}</div>{levels.map(([level, title, detail]) => <button className="dlevel" role="radio" aria-checked={permissions.level === level} disabled={p.saving} key={level} onClick={() => p.setLevel(level)}><span className="dradio" /><span className="tt"><b>{title}</b><small>{detail}</small></span></button>)}
      {permissions.actions.length > 0 && <><div className="dsec">Actions</div>{permissions.actions.map(action => { const tag = permissions.level === "read" ? "Off" : permissions.level === "allow" || action.allowed ? "Allowed" : "Asks"; return <div className="daction" key={action.tool}><span>{action.label}</span><span className={`dtag${tag === "Allowed" ? " ok" : tag === "Asks" ? " ask" : ""}`}>{tag}</span>{permissions.level === "ask" && <button type="button" className="dswitch" role="switch" aria-label={`Always allow ${action.label}`} aria-checked={action.allowed} disabled={p.saving} onClick={() => p.toggleAction(action.tool)} />}</div>; })}</>}
      <div className="dmuted dpad dblock">Searching and reading is always allowed.</div></>}
    <div className="dpad"><button className="dbtn warn" id="disconnect" disabled={p.saving} onClick={p.disconnect}>Disconnect {app.name}{(accounts?.length ?? 0) > 1 ? " (all accounts)" : ""}</button></div>
  </div></>;
}
