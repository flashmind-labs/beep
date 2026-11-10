import { Fragment, useCallback, useLayoutEffect, useRef } from "react";
import { bytes } from "../ui/format.ts";
import { CopyButton } from "../ui/kit.tsx";
import { machineTag, nodeBehind, coders, isCoder, shownVersion, updatedNotes, updateOffered, type NodeUpdate } from "./machines-state.ts";
import { signInOffer } from "./signin-state.ts";
import { relayLine, trafficRows, type MachineTraffic } from "./traffic-state.ts";
import type { Machine } from "./machines.ts";

interface MachineSearch { id: string; folder: string | null; draft: string | null; busy: boolean; error: string }
export interface MachineLog { id: string; text: string | null; busy: boolean; error: string }
interface ToolUpdate { machine_id: string; tool: string; latest: string }
interface Asking { id: string; busy: boolean; error: string }
interface DetailProps {
  machine: Machine; error: string; mac: string | null; newest: string | null; updatable: boolean;
  nodeUpdate: NodeUpdate | null; search: MachineSearch | null; log: MachineLog | null;
  tools: readonly ToolUpdate[]; toolUps: ReadonlyMap<string, { busy: boolean; error: string }>;
  installed: ReadonlyMap<string, string>; traffic: MachineTraffic | undefined; asking: Asking | null;
  age: (ms: number) => string;
  searchEdit: () => void; searchSave: (value: string) => void; searchCancel: () => void;
  signIn: (tool: string) => void; updateNode: () => void; updateTool: (tool: string) => void;
  logRead: () => void; logSave: () => void; askRemove: () => void; remove: () => void; cancelRemove: () => void;
}

export function MachineDetail(p: DetailProps) {
  const m = p.machine, env = m.env, tag = machineTag(m), up = p.nodeUpdate;
  const behind = m.id !== p.mac && nodeBehind(m.version, p.newest);
  const version = /^\d/.test(m.version) ? `beep-node ${m.version}` : m.version;
  const search = p.search?.id === m.id ? p.search : null;
  const log = p.log?.id === m.id ? p.log : null;
  const tools = env ? [...coders(env.tools), ...env.tools.filter(t => !isCoder(t.name))] : [];
  const notes = env ? updatedNotes(p.installed, m.id, env.tools) : [];
  const failures = [...p.toolUps].filter(([key, v]) => key.startsWith(`${m.id} `) && v.error);
  const traffic = trafficRows(p.traffic), relay = relayLine(p.traffic);
  const note = m.can_host === false ? "It disconnects from your mesh." : m.online ? "It stops taking work; its sessions and projects go with it." : "Its sessions and projects go with it. If it comes back online, it is turned away.";
  const asking = p.asking?.id === m.id ? p.asking : null;

  return <>
    {p.error && <div className="derr dpadx">{p.error}</div>}
    <div className="dset-box"><div className="dset-kv">
      <span>Status</span><span>{m.online ? <span className="dtag ok">Online</span> : <span className="dtag">Seen {p.age(Date.now() - m.seen_at)} ago</span>}{tag && <> <span className="dtag" title="Removed after a day offline">{tag}</span></>}</span>
      {m.can_host === false ? <><span>Device</span><span>Phone</span><span>Role</span><span>Controller</span></> : <><span>beep-node</span><span>{version}{behind && <> <span className="dtag ask">{p.newest} is out</span></>}{up?.step === "updating" ? <> <span className="dtag ask">Updating…</span></> : up?.step === "restarting" ? <> <span className="dtag ask">Restarting on {up.to}…</span></> : m.online && nodeBehind(m.version, p.newest) ? p.updatable ? <> <button type="button" className="dbtn" onClick={p.updateNode}>{up?.step === "failed" ? "Try again" : "Update"}</button></> : m.id !== p.mac ? <> <span className="dmuted">Run <span className="dmono">beep-node update</span> on the machine</span></> : null : null}</span></>}
      {env ? <><span>System</span><span>{env.os_version}, {env.arch}</span><span>Hardware</span><span>{env.cpus} CPUs, {Math.round(env.memory_mb / 1024)} GB</span><span>Host name</span><span><span className="dmono">{env.hostname}</span></span><span>Home</span><span><span className="dmono">{env.home}</span></span>
        {m.can_host !== false && search?.folder !== null && search?.folder !== undefined && <><span>Projects in</span><span>{search.draft === null ? <><span className="dmono">{search.folder}</span>{m.online && <> <button type="button" className="dbtn" onClick={p.searchEdit}>Change</button></>}</> : <SearchEditor key={m.id} draft={search.draft} busy={search.busy} save={p.searchSave} cancel={p.searchCancel} />}</span></>}
        {!!env.avds.length && <><span>Android emulators</span><span>{env.avds.join(", ")}</span></>}
      </> : m.can_host !== false && <><span>Report</span><span>None yet</span></>}
    </div></div>
    {up?.step === "failed" && <div className="derr dpadx">{up.error}</div>}
    {(up?.step === "note" || (up?.step === "restarting" && up.note)) && <div className="dmuted">{up.step === "note" ? up.text : up.note}</div>}
    {search?.error && <div className="derr dpadx">{search.error}</div>}
    {m.can_host !== false && !!env?.logins.length && <><div className="dset-h">Sign-ins</div><table className="dset-box dset-tbl"><thead><tr><th>Tool</th><th>Account</th><th /></tr></thead><tbody>{env.logins.map(l => { const offer = l.signed_in ? null : signInOffer(l.tool, m.name, m.online, m.id === p.mac); return <tr key={l.tool}><td className="dmono">{l.tool}</td><td>{l.signed_in ? l.account ?? "Signed in" : <span className="dtag ask">Not signed in</span>}</td><td style={{ textAlign: "right" }}>{offer && ("how" in offer ? <small className="dmuted">{offer.how}</small> : <button type="button" className="dbtn small" onClick={() => p.signIn(l.tool)}>Sign in</button>)}</td></tr>; })}</tbody></table></>}
    {m.can_host !== false && !!tools.length && <><div className="dset-hrow"><div className="dset-h">Tools</div>{tools.map(t => { const update = p.tools.find(u => u.machine_id === m.id && u.tool === t.name); if (!update || !m.online || !updateOffered(p.installed, `${m.id} ${t.name}`, update.latest)) return null; const busy = p.toolUps.get(`${m.id} ${t.name}`)?.busy; return <button key={t.name} type="button" className="dbtn small" disabled={busy} onClick={() => p.updateTool(t.name)}>{busy ? `Updating ${t.name}…` : `Update ${t.name} to ${update.latest}`}</button>; })}{notes.map((n, i) => <small key={i} className="dmuted">{n}</small>)}{failures.map(([key, v]) => <small key={key} className="derr">{`${key.slice(m.id.length + 1)}: ${v.error}`}</small>)}</div><table className="dset-box dset-tbl"><thead><tr><th>Tool</th><th>Version</th><th>Kind</th></tr></thead><tbody>{tools.map(t => <tr key={t.name}><td className="dmono">{t.name}</td><td className="dmono">{shownVersion(p.installed, `${m.id} ${t.name}`, t.name, t.version) ?? ""}</td><td>{isCoder(t.name) ? "Coding agent" : "Tool"}</td></tr>)}</tbody></table></>}
    {!!traffic.length && <><div className="dset-h">Traffic</div><table className="dset-box dset-tbl"><thead><tr><th>Protocol</th><th>Sent</th><th>Received</th></tr></thead><tbody>{traffic.map(r => <tr key={r.label}><td>{r.label}</td><td className="dmono">{bytes(r.sent)}</td><td className="dmono">{bytes(r.received)}</td></tr>)}</tbody></table>{relay && <div className="dmuted">Through a relay: {relay}.</div>}</>}
    {m.can_host !== false && (log || m.online) && <LogSection key={m.id} log={log} read={p.logRead} save={p.logSave} />}
    <div className="dmach dset-rm">{m.id !== p.mac && !asking && <div className="dset-rm-row"><span className="t"><b>Remove this machine</b><small>{note}</small></span><button type="button" className="dbtn warn" aria-label={`Remove ${m.name}`} onClick={p.askRemove}>Remove…</button></div>}{asking && m.id !== p.mac && <div className="ask"><span>Remove {m.name} from your account? {note} {m.can_host === false ? "To reconnect, pair it again." : "To use it again, add it with a new setup command."}</span>{asking.error && <span className="derr">{asking.error}</span>}<span className="acts"><button type="button" className="dbtn warn" disabled={asking.busy} onClick={p.remove}>Remove</button><button type="button" className="dbtn quiet" onClick={p.cancelRemove}>Cancel</button></span></div>}</div>
  </>;
}

function SearchEditor({ draft, busy, save, cancel }: { draft: string; busy: boolean; save: (value: string) => void; cancel: () => void }) {
  const input = useRef<HTMLInputElement>(null);
  useLayoutEffect(() => { input.current?.focus(); }, []);
  return <span className="dfield dline tight"><input ref={input} className="dmono" defaultValue={draft} aria-label="Search for projects in" spellCheck={false} disabled={busy} onKeyDown={e => { if (e.key === "Enter") save(e.currentTarget.value); if (e.key === "Escape") { e.stopPropagation(); cancel(); } }} /><button type="button" className="dbtn pri" disabled={busy} onClick={() => { if (input.current) save(input.current.value); }}>Save</button><button type="button" className="dbtn quiet" onClick={cancel}>Cancel</button></span>;
}

function LogSection({ log, read, save }: { log: MachineLog | null; read: () => void; save: () => void }) {
  const pre = useRef<HTMLPreElement>(null), bottom = useRef(true);
  const attach = useCallback((element: HTMLPreElement | null) => {
    pre.current = element;
    if (element && bottom.current) element.scrollTop = element.scrollHeight;
  }, []);
  useLayoutEffect(() => { if (pre.current && bottom.current) pre.current.scrollTop = pre.current.scrollHeight; }, [log?.text]);
  const readButton = (label: string) => <button type="button" className="dbtn" disabled={log?.busy} onClick={read}>{label}</button>;
  let body = log ? null : readButton("Show");
  if (log?.busy && log.text === null) body = <div className="dmuted">Reading…</div>;
  else if (log?.error) body = <><div className="derr dpadx">{log.error}</div>{readButton("Try again")}</>;
  else if (log?.text === "") body = <><div className="dmuted">Nothing logged lately.</div>{readButton("Refresh")}</>;
  else if (log?.text) body = <><pre ref={attach} className="dlog dmono" onScroll={e => { const el = e.currentTarget; bottom.current = el.scrollHeight - el.scrollTop - el.clientHeight < 4; }}>{log.text}</pre><div className="dlog-acts">{readButton("Refresh")}<CopyButton text={log.text} /><button type="button" className="dbtn" onClick={save}>Save</button></div></>;
  return <Fragment><div className="dset-h">Log</div>{body}</Fragment>;
}
