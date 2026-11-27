import { Fragment, useCallback, useLayoutEffect, useRef } from "react";
import type { invoke as Invoke } from "../../ui/tauri.ts";
import { bytes } from "../../ui/format.ts";
import { Button, CopyButton, NO_AUTO } from "../../ui/kit.tsx";
import { gpuLine, machineTag, nodeBehind, coders, isCoder, nodeVersion, removalNote, shownVersion, updatedNotes, updateOffered, type NodeUpdate } from "../../stores/machines/machines-state.ts";
import { signInOffer } from "../../stores/machines/signin-state.ts";
import { relayLine, trafficRows, type MachineTraffic } from "../../stores/machines/traffic-state.ts";
import { MachineMcpServers } from "./mcp-servers-view.tsx";
import { RemoveAsk } from "./list-view.tsx";
import { WslRows } from "./wsl-view.tsx";
import { wslHosts } from "../../stores/machines/wsl-state.ts";
import type { Asking, Machine, MachineSearch, ToolUpdate } from "./machines.tsx";

export interface MachineLog { id: string; text: string | null; busy: boolean; error: string }
interface DetailProps {
  machine: Machine; error: string; mac: string | null; newest: string | null; updatable: boolean;
  nodeUpdate: NodeUpdate | null; search: MachineSearch | null; log: MachineLog | null;
  tools: readonly ToolUpdate[]; toolUps: ReadonlyMap<string, { busy: boolean; error: string }>;
  installed: ReadonlyMap<string, string>; traffic: MachineTraffic | undefined; asking: Asking | null;
  age: (ms: number) => string;
  invoke: typeof Invoke;
  searchEdit: () => void; searchSave: (value: string) => void; searchCancel: () => void;
  signIn: (tool: string) => void; updateNode: () => void; updateTool: (tool: string) => void;
  logRead: () => void; logSave: () => void; rename: () => void; askRemove: () => void; remove: () => void; cancelRemove: () => void;
}

export function MachineDetail(p: DetailProps) {
  const m = p.machine, env = m.env, tag = machineTag(m), up = p.nodeUpdate;
  const behind = m.id !== p.mac && nodeBehind(m.version, p.newest);
  const version = nodeVersion(m.version);
  const search = p.search?.id === m.id ? p.search : null;
  const log = p.log?.id === m.id ? p.log : null;
  const tools = env ? [...coders(env.tools), ...env.tools.filter(t => !isCoder(t.name))] : [];
  const notes = env ? updatedNotes(p.installed, m.id, env.tools) : [];
  const failures = [...p.toolUps].filter(([key, v]) => key.startsWith(`${m.id} `) && v.error);
  const traffic = trafficRows(p.traffic), relay = relayLine(p.traffic);
  const note = removalNote(m);
  const asking = p.asking?.id === m.id ? p.asking : null;

  return <>
    {p.error && <div className="derr dpadx">{p.error}</div>}
    <div className="dset-box"><div className="dset-kv">
      <span>Name</span><span>{m.name} <Button onClick={p.rename}>Rename</Button></span>
      <span>Status</span><span>{m.online ? <span className="dtag ok">Online</span> : <span className="dtag">Seen {p.age(Date.now() - m.seen_at)} ago</span>}{tag && <> <span className="dtag" title="Removed after a day offline">{tag}</span></>}</span>
      {m.can_host === false ? <><span>Device</span><span>Phone</span><span>Role</span><span>Controller</span></> : <><span>beep-node</span><span>{version}{behind && <> <span className="dtag ask">{p.newest} is out</span></>}{up?.step === "updating" ? <> <span className="dtag ask">Updating…</span></> : up?.step === "restarting" ? <> <span className="dtag ask">Restarting on {up.to}…</span></> : m.online && nodeBehind(m.version, p.newest) ? p.updatable ? <> <Button onClick={p.updateNode}>{up?.step === "failed" ? "Try again" : "Update"}</Button></> : m.id !== p.mac ? <> <span className="dmuted">Run <span className="dmono">beep-node update</span> on the machine</span></> : null : null}</span></>}
      {env ? <><span>System</span><span>{env.os_version}, {env.arch}</span><span>Hardware</span><span>{env.cpus} CPUs, {Math.round(env.memory_mb / 1024)} GB</span>{!!env.gpus.length && <><span>Graphics</span><span>{gpuLine(env.gpus)}</span></>}<span>Host name</span><span><span className="dmono">{env.hostname}</span></span><span>Home</span><span><span className="dmono">{env.home}</span></span>
        {m.can_host !== false && search?.folder !== null && search?.folder !== undefined && <><span>Projects in</span><span>{search.draft === null ? <><span className="dmono">{search.folder}</span>{m.online && <> <Button onClick={p.searchEdit}>Change</Button></>}</> : <SearchEditor key={m.id} draft={search.draft} busy={search.busy} save={p.searchSave} cancel={p.searchCancel} />}</span></>}
        {!!env.avds.length && <><span>Android emulators</span><span>{env.avds.join(", ")}</span></>}
      </> : m.can_host !== false && <><span>Report</span><span>None yet</span></>}
    </div></div>
    {up?.step === "failed" && <div className="derr dpadx">{up.error}</div>}
    {(up?.step === "note" || (up?.step === "restarting" && up.note)) && <div className="dmuted">{up.step === "note" ? up.text : up.note}</div>}
    {search?.error && <div className="derr dpadx">{search.error}</div>}
    {wslHosts([m]).map(host => <Fragment key={host.id}><div className="dset-h">WSL</div><div className="dset-box"><div className="dmach"><WslRows host={host} /></div></div></Fragment>)}
    {m.can_host !== false && !!env?.logins.length && <><div className="dset-h">Sign-ins</div><table className="dset-box dset-tbl"><thead><tr><th>Tool</th><th>Account</th><th /></tr></thead><tbody>{env.logins.map(l => { const offer = l.signed_in ? null : signInOffer(l.tool, m.name, m.online, m.id === p.mac); return <tr key={l.tool}><td className="dmono">{l.tool}</td><td>{l.signed_in ? l.account ?? "Signed in" : <span className="dtag ask">Not signed in</span>}</td><td style={{ textAlign: "right" }}>{offer && ("how" in offer ? <small className="dmuted">{offer.how}</small> : <Button small onClick={() => p.signIn(l.tool)}>Sign in</Button>)}</td></tr>; })}</tbody></table></>}
    {m.can_host !== false && !!tools.length && <><div className="dset-hrow"><div className="dset-h">Tools</div>{tools.map(t => { const update = p.tools.find(u => u.machine_id === m.id && u.tool === t.name); if (!update || !m.online || !updateOffered(p.installed, `${m.id} ${t.name}`, update.latest)) return null; const busy = p.toolUps.get(`${m.id} ${t.name}`)?.busy; return <Button key={t.name} small disabled={busy} onClick={() => p.updateTool(t.name)}>{busy ? `Updating ${t.name}…` : `Update ${t.name} to ${update.latest}`}</Button>; })}{notes.map((n, i) => <small key={i} className="dmuted">{n}</small>)}{failures.map(([key, v]) => <small key={key} className="derr">{`${key.slice(m.id.length + 1)}: ${v.error}`}</small>)}</div><table className="dset-box dset-tbl"><thead><tr><th>Tool</th><th>Version</th><th>Kind</th></tr></thead><tbody>{tools.map(t => <tr key={t.name}><td className="dmono">{t.name}</td><td className="dmono">{shownVersion(p.installed, `${m.id} ${t.name}`, t.name, t.version) ?? ""}</td><td>{isCoder(t.name) ? "Coding agent" : "Tool"}</td></tr>)}</tbody></table></>}
    {m.can_host !== false && <MachineMcpServers key={`mcp:${m.id}`} machineId={m.id} online={m.online} invoke={p.invoke} />}
    {!!traffic.length && <><div className="dset-h">Traffic</div><table className="dset-box dset-tbl"><thead><tr><th>Protocol</th><th>Sent</th><th>Received</th></tr></thead><tbody>{traffic.map(r => <tr key={r.label}><td>{r.label}</td><td className="dmono">{bytes(r.sent)}</td><td className="dmono">{bytes(r.received)}</td></tr>)}</tbody></table>{relay && <div className="dmuted">Through a relay: {relay}.</div>}</>}
    {m.can_host !== false && (log || m.online) && <LogSection key={m.id} log={log} read={p.logRead} save={p.logSave} />}
    <div className="dmach dset-rm">{m.id !== p.mac && !asking && <div className="dset-rm-row"><span className="t"><b>Remove this machine</b><small>{note}</small></span><Button danger aria-label={`Remove ${m.name}`} onClick={p.askRemove}>Remove…</Button></div>}{asking && m.id !== p.mac && <RemoveAsk machine={m} asking={asking} remove={p.remove} cancel={p.cancelRemove} />}</div>
  </>;
}

function SearchEditor({ draft, busy, save, cancel }: { draft: string; busy: boolean; save: (value: string) => void; cancel: () => void }) {
  const input = useRef<HTMLInputElement>(null);
  useLayoutEffect(() => { input.current?.focus(); }, []);
  return <span className="dfield dline tight"><input ref={input} className="dmono" defaultValue={draft} aria-label="Search for projects in" {...NO_AUTO} disabled={busy} onKeyDown={e => { if (e.key === "Enter") save(e.currentTarget.value); if (e.key === "Escape") { e.stopPropagation(); cancel(); } }} /><Button look="primary" disabled={busy} onClick={() => { if (input.current) save(input.current.value); }}>Save</Button><Button look="quiet" onClick={cancel}>Cancel</Button></span>;
}

function LogSection({ log, read, save }: { log: MachineLog | null; read: () => void; save: () => void }) {
  const pre = useRef<HTMLPreElement>(null), bottom = useRef(true);
  const attach = useCallback((element: HTMLPreElement | null) => {
    pre.current = element;
    if (element && bottom.current) element.scrollTop = element.scrollHeight;
  }, []);
  useLayoutEffect(() => { if (pre.current && bottom.current) pre.current.scrollTop = pre.current.scrollHeight; }, [log?.text]);
  const readButton = (label: string) => <Button disabled={log?.busy} onClick={read}>{label}</Button>;
  let body = log ? null : readButton("Show");
  if (log?.busy && log.text === null) body = <div className="dmuted">Reading…</div>;
  else if (log?.error) body = <><div className="derr dpadx">{log.error}</div>{readButton("Try again")}</>;
  else if (log?.text === "") body = <><div className="dmuted">Nothing logged lately.</div>{readButton("Refresh")}</>;
  else if (log?.text) body = <><pre ref={attach} className="dlog dmono" onScroll={e => { const el = e.currentTarget; bottom.current = el.scrollHeight - el.scrollTop - el.clientHeight < 4; }}>{log.text}</pre><div className="dlog-acts">{readButton("Refresh")}<CopyButton text={log.text} /><Button onClick={save}>Save</Button></div></>;
  return <Fragment><div className="dset-h">Log</div>{body}</Fragment>;
}
