import type { CSSProperties } from "react";
import { Icon } from "../ui/icons.tsx";
import { agentOf, count, doing, kind, round, type Agent, type Entry, type Open } from "./research-state.ts";
import { Avatar } from "./avatar-view.tsx";
import { Citations } from "./live-pane-view.tsx";

export function ResearchAgents({ research: o }: { research: Open }) {
  const r = o.research, agents = r?.agents ?? [{ id: "lead", name: "Lead", role: "Plans and coordinates", model: "", round: 0 }];
  const rounds = round(o), pick = Math.min(o.pick ?? rounds, rounds);
  const scoutOf = (a: Agent) => kind(a.id) === "scout" ? Math.max(a.round ?? 0, 1) : 0;
  const rows = [...agents.filter(a => a.id === "lead"), ...agents.filter(a => scoutOf(a) === pick), ...agents.filter(a => a.id === "critic" || a.id === "writer")];
  const scouts = (n: number) => count(agents.filter(a => scoutOf(a) === n).length, "scout");
  const said = o.entries.filter(e => e.kind === "said").length;
  const roomStyle: CSSProperties & { "--c": string } = { "--c": "var(--muted)" };
  return <>{rounds > 1 && <div className="rhd"><h4>Agents</h4><span className="rround"><button className="dbtn small" data-act="rounds" aria-haspopup="menu" aria-expanded={o.menu}>Round {pick}<svg width="10" height="10" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="3" strokeLinecap="round" strokeLinejoin="round"><path d="M6 9l6 6 6-6" /></svg></button>{o.menu && <span className="dmenu" role="menu">{Array.from({ length: rounds }, (_, i) => rounds - i).map(n => <button key={n} className="mi" role="menuitemradio" aria-checked={n === pick} data-act={`round:${n}`}><b>Round {n}</b>{n === pick && <span className="ok"><Icon name="check" /></span>}<small>{scouts(n)}</small></button>)}</span>}</span></div>}
    <div className="rpane agents"><div className="rbox">{rows.map(a => { const [now, state] = doing(o, a.id); return <button key={a.id} className="rag" data-act={`agent:${a.id}`}><Avatar id={a.id} /><b>{a.name}</b>{state === "run" ? <span className="dspin" /> : state === "ok" ? <span className="dok"><Icon name="check" /></span> : state === "wait" ? <span className="ddot hot" /> : <span />}<small>{now || a.role}</small><span className="model">{a.model}</span></button>; })}</div><div className="rbox"><button className="rag" data-act="room"><span className="rav" style={roomStyle}>#</span><b>Room</b><span /><small>{r && r.status !== "planning" ? count(said, "message") : "Opens when the team starts"}</small></button></div></div></>;
}
type Part = { kind: "steps"; entries: (Entry & { kind: "step" })[] } | { kind: "item"; entry: Entry };
export function ResearchThread({ research: o, agent }: { research: Open; agent: Agent }) {
  const parts: Part[] = [];
  for (const entry of o.entries.filter(e => e.agent === agent.id || (e.kind === "said" && e.to === agent.id))) {
    if (entry.kind === "step" && entry.agent === agent.id) {
      const last = parts.at(-1);
      if (last?.kind === "steps") last.entries.push(entry); else parts.push({ kind: "steps", entries: [entry] });
    } else if (entry.kind === "said" || entry.kind === "note") parts.push({ kind: "item", entry });
  }
  const [now, state] = doing(o, agent.id);
  return <><div className="rhd"><Avatar id={agent.id} /><span className="who"><b>{agent.name}</b><small>{agent.role} · {agent.model}</small></span></div><div className="rpane thread">{parts.map(part => part.kind === "steps" ? <div className="dsteps" key={`steps:${part.entries[0].seq}`}>{part.entries.map(e => <div className="dstep" key={e.seq}><span className="s">{e.success ? <span className="dok"><Icon name="check" /></span> : <span className="dbad"><Icon name="x" /></span>}</span><span>{e.label}</span></div>)}</div> : part.entry.kind === "said" ? <div className="rmsg" key={part.entry.seq}><Avatar id={part.entry.agent} /><b>{agentOf(o, part.entry.agent).name}{part.entry.to && <> <span className="to">to {agentOf(o, part.entry.to).name}</span></>}</b><p>{part.entry.content}</p></div> : part.entry.kind === "note" ? <div className="rmsg" key={part.entry.seq}><Avatar id={part.entry.agent} /><b>Note</b><p>{part.entry.content}{!!part.entry.sources.length && <> <Citations research={o} numbers={part.entry.sources} /></>}</p></div> : null)}{state === "run" && <div className="dsteps"><div className="dstep run"><span className="s"><span className="dspin" /></span><span>{now}</span></div></div>}{!parts.length && state !== "run" && <div className="dmuted">Nothing yet.</div>}</div></>;
}
