import { HintView, type HintViewModel } from "../../parts/bar/hints-view.tsx";
import type { CSSProperties } from "react";
import { HarnessIcon, Icon } from "../../ui/icons.tsx";
import { bareVersion, coderColumns, coders, machineTag, nodeBehind, nodeVersion, removalNote, specLine, toolChips } from "../../stores/machines/machines-state.ts";
import { adapterLine } from "../../stores/machines/adapters-state.ts";
import type { Asking, Machine } from "./machines.tsx";
import { clockWarning } from "../../stores/machines/clock.ts";
import { Button, onRowKey, SkeletonRows } from "../../ui/kit.tsx";

interface ListProps {
  list: Machine[] | null;
  error: string;
  hosted: boolean;
  mac: string | null;
  newest: string | null;
  asking: Asking | null;
  adapterTries: Map<string, { busy: boolean; error: string }>;
  hints: HintViewModel | null;
  age: (milliseconds: number) => string;
  open: (id: string) => void;
  add: () => void;
  link: () => void;
  askRemove: (id: string) => void;
  cancelRemove: () => void;
  remove: () => void;
  retryAdapter: (id: string) => void;
}

type ColumnStyle = CSSProperties & { "--ag": number };

export function MachineList(p: ListProps) {
  const list = p.list ?? [], on = list.filter(m => m.online).length;
  const summary = p.list ? [on && `${on} online`, list.length - on && `${list.length - on} offline`].filter(Boolean).join(", ") : "";
  const say = summary || "Where Beep runs commands and coding agents";
  const columns = coderColumns(list.map(m => m.env?.tools ?? []));
  const style: ColumnStyle = { "--ag": columns.length };
  const actions = <span className="dline"><Button look="quiet" id="mcLinkDesktop" onClick={p.link}>Link another desktop</Button><Button id="mcAdd" onClick={p.add}>Add a machine</Button></span>;
  return <>
    {p.hosted ? <div className="dmc-head"><span className="dmuted">{say}</span>{actions}</div> : <div className="dhead"><span className="tt"><b>Machines</b><small>{say}</small></span>{actions}</div>}
    {p.error && <div className="derr dpadx">{p.error}</div>}
    {p.list && !list.length ? <div className="dempty"><span className="dicon"><svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round"><rect x="3" y="4" width="18" height="7" rx="2" /><rect x="3" y="13" width="18" height="7" rx="2" /><path d="M7 7.5h.01M7 16.5h.01" /></svg></span><b className="dbig">No machines yet.</b><span className="dmuted dblock">Add one to run commands and coding agents on it.</span></div> : <div className={`dlist${p.hosted ? " dset-box" : ""}`}><div className="dmt" style={style}>{list.map(m => <MachineCard key={m.id} machine={m} columns={columns} props={p} />)}</div></div>}
    {!p.hosted && p.hints && <HintView value={p.hints} />}
  </>;
}

/// The question under a machine that waits for Remove's answer; `ids` gives
/// its buttons ids.
export function RemoveAsk({ machine: m, asking, ids = false, remove, cancel }: { machine: Machine; asking: Asking; ids?: boolean; remove: () => void; cancel: () => void }) {
  const note = removalNote(m);

  return <div className="ask"><span>Remove {m.name} from the mesh? {note} {m.can_host === false ? "To reconnect, pair it again." : "To use it again, add it with a new setup command."}</span>{asking.error && <span className="derr">{asking.error}</span>}<span className="acts"><Button danger id={ids ? "rmYes" : undefined} disabled={asking.busy} onClick={remove}>Remove</Button><Button look="quiet" id={ids ? "rmNo" : undefined} autoFocus onClick={cancel}>Cancel</Button></span></div>;
}

function MachineCard({ machine: m, columns, props: p }: { machine: Machine; columns: string[]; props: ListProps }) {
  const env = m.env, tag = machineTag(m), clock = clockWarning(m.id, env?.os ?? null);
  const spec = m.can_host === false ? "Phone · Controller" : env ? specLine(env) : "No report yet";
  const version = nodeVersion(m.version);
  const behind = m.id !== p.mac && nodeBehind(m.version, p.newest);
  const asking = p.asking?.id === m.id ? p.asking : null;
  const tried = p.adapterTries.get(m.id);
  const agents = env ? coders(env.tools) : [];
  const tools = env ? toolChips(env.tools) : { shown: [], more: [] };
  const open = () => p.open(m.id);
  return <div className="dmach" data-machine={p.hosted ? m.id : undefined} role={p.hosted ? "button" : undefined} tabIndex={p.hosted ? 0 : undefined} aria-label={p.hosted ? m.name : undefined} onClick={p.hosted ? open : undefined} onKeyDown={p.hosted ? onRowKey(open) : undefined}>
    <span className="l1"><span className={`ddot${m.online ? "" : " off"}`} title={m.online ? "Online" : "Offline"} /><b>{m.name}</b>
      {tag && <span className="dtag" title="Removed after a day offline">{tag}</span>}
      {!m.online && <span className="seen">Seen {p.age(Date.now() - m.seen_at)} ago</span>}
      {clock && <span className="clockoff" title={clock.title}>{clock.text}</span>}
      <span className="spec" title={spec}>{spec}</span>
      {m.can_host !== false && <span className="dmnv" title={behind ? `beep-node ${m.version}; ${p.newest} is out` : version}>{version}{behind && <span className="up">update</span>}</span>}
      {!p.hosted && m.id !== p.mac && !asking && <Button danger aria-label={`Remove ${m.name}`} onClick={() => p.askRemove(m.id)}>Remove…</Button>}
    </span>
    {p.hosted && <span className="ch"><Icon name="chev" /></span>}
    {env && m.can_host !== false && <>
      {agents.length ? agents.map(t => { const v = bareVersion(t.name, t.version) ?? "", login = env.logins.find(l => l.tool === t.name); const title = `${t.name} ${v}${!login ? "" : login.signed_in ? ", signed in" : ", not signed in"}`; return <span key={t.name} className="dmag" style={{ gridColumn: columns.indexOf(t.name) + 1 }} title={title}><HarnessIcon agent={t.name} className="dmag-ic" /><span className="v">{v}</span>{login && <span className={login.signed_in ? "in" : "out"} />}</span>; }) : <span className="none">No coding agents</span>}
      {!!tools.shown.length && <span className="l3">{tools.shown.map(n => <span className="dmt-t" key={n}>{n}</span>)}{!!tools.more.length && <span className="dmt-t" title={tools.more.join(", ")}>+{tools.more.length}</span>}</span>}
      {m.online && (m.adapters ?? []).map((a, i) => { const text = adapterLine(a); return <span key={`${a.agent}-${i}`} className={`dmad${a.failed === null ? "" : " bad"}`}><span title={text}>{text}</span>{a.failed !== null && <Button small disabled={tried?.busy} onClick={e => { e.stopPropagation(); p.retryAdapter(m.id); }}>{tried?.busy ? "Trying again…" : "Try again"}</Button>}</span>; })}
      {m.online && tried?.error && <span className="dmad bad"><span>{tried.error}</span></span>}
    </>}
    {asking && !p.hosted && <RemoveAsk machine={m} asking={asking} ids remove={p.remove} cancel={p.cancelRemove} />}
  </div>;
}


export function MachineLoading() {
  return <SkeletonRows widths={[78, 56, 68]} />;
}
