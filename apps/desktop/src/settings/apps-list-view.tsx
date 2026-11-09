import { HintView, type HintViewModel } from "../bar/hints-view.tsx";
import type { ReactNode } from "react";
import { useLayoutEffect, useRef, useState, type KeyboardEvent } from "react";
import { ICON } from "../ui/icons.ts";
import { plural } from "../ui/format.ts";
import { hostOf, type App, type AppAccount, type McpServer } from "./apps.ts";

interface Props {
  apps: App[] | null; accounts: Record<string, AppAccount[]>; error: string; connecting: string | null;
  servers: McpServer[] | null; waiting: string | null; busy: string | null; adding: boolean; addError: string; serverName: string; serverUrl: string; changeName(value: string): void; changeUrl(value: string): void;
  add(): void; startAdd(): void; cancelAdd(): void; cancelServer(): void; connectServer(id: string): void; removeServer(id: string): void; openServer(id: string): void;
  connectApp(slug: string): void; openApp(slug: string): void; cancelApp(): void;
}
function Logo({ app }: { app: App }) {
  const [failed, setFailed] = useState<string | null>(null), url = app.slug === "fastmail" ? "fastmail.svg" : app.logo_url;
  return url && failed !== url ? <img className="dalogo" src={url} alt="" onError={() => setFailed(url)} /> : <span className="dalogo">{app.name.slice(0, 1).toUpperCase()}</span>;
}
function Chevron() { return <span className="dmuted" dangerouslySetInnerHTML={{ __html: ICON.chev }} />; }
function Waiting({ cancel }: { cancel: () => void }) { return <><span className="dmuted"><span className="dspin" /> Waiting for browser…</span><button className="dbtn quiet" onClick={e => { e.stopPropagation(); cancel(); }}>Cancel</button></>; }
function openOnKey(event: KeyboardEvent<HTMLElement>, action: () => void) { if (event.target === event.currentTarget && (event.key === "Enter" || event.key === " ")) { event.preventDefault(); action(); } }
function AppRow({ app, p }: { app: App; p: Props }) {
  const accounts = p.accounts[app.slug], accountLine = accounts?.length ? accounts.map(a => a.address ?? `${app.name} account`).join(", ") : null;
  const open = () => p.openApp(app.slug);
  return <div className={`dappr${app.connected ? " on" : ""}`} data-open={app.connected ? app.slug : undefined} role={app.connected ? "button" : undefined} tabIndex={app.connected ? 0 : undefined} onClick={app.connected ? open : undefined} onKeyDown={app.connected ? e => openOnKey(e, open) : undefined}>
    <Logo app={app} /><span className="tt"><b>{app.name}</b><small>{app.connected && accountLine || app.description}</small></span>
    {p.connecting === app.slug ? <Waiting cancel={p.cancelApp} /> : app.connected ? <><span className="dtag">Connected</span><Chevron /></> : !app.configured ? <span className="dmuted">Unavailable</span> : <button className="dbtn pri" disabled={!!p.connecting} onClick={() => p.connectApp(app.slug)}>Connect</button>}
  </div>;
}
function ServerRow({ server, p }: { server: McpServer; p: Props }) {
  const openable = server.connected && p.waiting !== server.id && p.busy !== server.id, open = () => p.openServer(server.id);
  return <div className={`dappr${openable ? " on" : ""}`} data-mopen={openable ? server.id : undefined} role={openable ? "button" : undefined} tabIndex={openable ? 0 : undefined} onClick={openable ? open : undefined} onKeyDown={openable ? e => openOnKey(e, open) : undefined}>
    <span className="dalogo">{server.name.slice(0, 1).toUpperCase()}</span><span className="tt"><b>{server.name}</b><small>{hostOf(server.url)}</small></span>
    {p.waiting === server.id ? <Waiting cancel={p.cancelServer} /> : p.busy === server.id ? <span className="dmuted"><span className="dspin" /></span> : server.connected ? <><span className="dtag ok">{plural(server.tools.length, "tool", "tools")}</span><Chevron /></> : <><span className="dtag ask">{server.sign_in ? "Sign-in expired" : "Not connected"}</span><button className="dbtn pri" disabled={!!p.busy} onClick={() => p.connectServer(server.id)}>{server.sign_in ? "Sign in" : "Retry"}</button><button className="dbtn quiet warn" disabled={!!p.busy} onClick={() => p.removeServer(server.id)}>Remove</button></>}
  </div>;
}
function AddServer({ p }: { p: Props }) {
  const name = useRef<HTMLInputElement>(null), busy = p.busy === "new";
  useLayoutEffect(() => { name.current?.focus(); }, []);
  return <form className="dform dmcp" onSubmit={e => { e.preventDefault(); p.add(); }}>
    <label className="dfield"><span>Name</span><input ref={name} id="mname" maxLength={60} placeholder="Team notes" autoComplete="off" spellCheck={false} defaultValue={p.serverName} onChange={e => p.changeName(e.currentTarget.value)} disabled={busy} /></label>
    <label className={`dfield${p.addError ? " err" : ""}`}><span>{p.addError || "Server URL"}</span><input id="murl" className="url" maxLength={2000} placeholder="https://example.com/mcp" autoComplete="off" spellCheck={false} defaultValue={p.serverUrl} onChange={e => p.changeUrl(e.currentTarget.value)} disabled={busy} /></label>
    <div className="dline">{busy ? <span className="dmuted"><span className="dspin" /> Connecting…</span> : <><button type="submit" className="dbtn pri">Add server<kbd dangerouslySetInnerHTML={{ __html: ICON.ret }} /></button><button type="button" className="dbtn quiet" onClick={p.cancelAdd}>Cancel</button></>}</div>
  </form>;
}
export function AppsListView(p: Props) {
  const connected = (p.apps ?? []).filter(app => app.connected), more = (p.apps ?? []).filter(app => !app.connected);
  return <div className="dlist dapps" id="alist">{p.error ? <div className="derr dpad">{p.error}</div> : !p.apps && <div className="dmuted dpad"><span className="dspin" /></div>}
    {!!connected.length && <><div className="dsec">Connected</div>{connected.map(app => <AppRow key={app.slug} app={app} p={p} />)}</>}
    {!!more.length && <><div className="dsec">{connected.length ? "More apps" : "Available"}</div>{more.map(app => <AppRow key={app.slug} app={app} p={p} />)}</>}
    {p.servers && <><div className="dsec">Your MCP servers</div>{p.servers.map(server => <ServerRow key={server.id} server={server} p={p} />)}{p.adding ? <AddServer p={p} /> : <div className="dpad"><button className="dbtn" onClick={p.startAdd}>Add MCP server</button></div>}<div className="dmuted dpad dblock">Beep asks before every tool on a server you add.</div></>}
  </div>;
}

export function AppBarFrame({ section, hints, children }: { section: "apps" | "mcp" | "appperms"; hints: HintViewModel | null; children: ReactNode }) {
  return <>{section === "apps" && <div className="dhead"><span className="tt"><b>Apps</b><small>Connect once, then just ask from any chat</small></span></div>}{children}{hints && <HintView value={hints} />}</>;
}
