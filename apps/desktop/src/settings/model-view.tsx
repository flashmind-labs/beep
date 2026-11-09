import { HintView, type HintViewModel } from "../bar/hints-view.tsx";
import type { ModelChoice, Models, Thinking } from "./model.ts";

interface ModelViewProps {
  model: Models;
  selected: number;
  agents: boolean;
  agentNames: readonly string[];
  error: string;
  hints: HintViewModel;
  thinkingLevels: readonly Thinking[];
  price: (choice: ModelChoice) => string;
  capitalize: (value: string) => string;
  pick: (index: number) => void;
  setThinking: (level: Thinking) => void;
  setAuto: () => void;
  setAgents: () => void;
}


export function ModelView(p: ModelViewProps) {
  const m = p.model;
  const autoSub = m.auto ? (p.agents ? "Jev picks an agent for each prompt" : "Jev picks the thinking for each prompt") : "Off";
  return <>
    <div className="dhead"><span className="tt"><b>Model</b><small>Your own turns only</small></span></div>
    <div className="dform dmodelform">
      {m.auto_available && <div className="dtoggle"><span><b>Auto</b><small>{autoSub}</small></span><button type="button" className="dswitch" role="switch" aria-checked={m.auto} aria-label="Auto" onClick={e => { e.stopPropagation(); p.setAuto(); }} /></div>}
      <div className="dlist dflush" role="listbox" aria-label="Models">{m.choices.map((c, i) => <button type="button" className="drt dmrow" role="option" key={c.id} aria-selected={i === p.selected} onClick={e => { e.stopPropagation(); p.pick(i); }}><span className="tt"><b>{c.name}</b><span>{p.price(c)}</span></span>{c.id === m.selected && <span className="dok"><svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="3" strokeLinecap="round" strokeLinejoin="round"><path d="M5 12.5l4.5 4.5L19 7.5" /></svg></span>}</button>)}</div>
      <div className="dfield"><span>Thinking</span><div className="dchips">{p.thinkingLevels.map(level => <button type="button" key={level} className="dchip" aria-pressed={level === m.thinking} onClick={e => { e.stopPropagation(); p.setThinking(level); }}>{p.capitalize(level)}</button>)}</div></div>
      {!!p.agentNames.length && <><div className="dsec flat dagents">Agents <span className="dtag">Admin</span></div><div className="dtoggle"><span><b>Agents</b><small>{p.agents ? "On for everyone" : "Off for everyone"}</small></span><button type="button" className="dswitch" role="switch" aria-checked={p.agents} aria-label="Agents" onClick={e => { e.stopPropagation(); p.setAgents(); }} /></div><div className="dmuted dagentlist" style={p.agents ? undefined : { opacity: 0.6 }}>{p.agentNames.join(", ")}</div></>}
      <div className="derr" id="modelErr">{p.error}</div>
    </div>
    <HintView value={p.hints} />
  </>;
}
