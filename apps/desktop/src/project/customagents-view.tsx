import { useLayoutEffect, useRef } from "react";
import { agentLabel } from "./projects-state.ts";
import { HarnessIcon } from "../ui/icons.tsx";
import { Button, EmptyPanel, SectionHead, onRowKey, SheetHead } from "../ui/kit.tsx";
import { agentColor, agentMeta, type CustomAgentRow } from "./customagents-state.ts";
import type { Persona } from "../agents/agents-state.ts";

/// The custom agent a session runs as, in its row's detail line or its head: its colour's dot
/// and its name. Nothing for a session that runs as none.
export function PersonaTag({ persona }: { persona: Persona | null | undefined }) {
  if (!persona) return null;
  const color = agentColor(persona.color);
  return <span className="th-persona" title={`Runs as the custom agent ${persona.name}`}>{color && <span className="ddot" style={{ background: color }} />}{persona.name}</span>;
}

export function CustomAgentsView({ rows, message, error, reload, open, start }: { rows: CustomAgentRow[] | null; message: string; error: boolean; reload: (() => void) | null; open: (row: CustomAgentRow) => void; start: ((row: CustomAgentRow) => void) | null }) {
  return <><SectionHead title="Custom agents" count={rows?.length ?? 0}>{reload && <Button look="quiet" small onClick={reload}>Reload</Button>}</SectionHead>
    {rows?.length ? <div className="cc-panel pv-scroll">{rows.map(a => {
      if (a.error && !a.description && !a.prompt) return <div key={`${a.harness}:${a.path}`} className="pj-task ca-row bad"><span className="t"><span className="ddot bad" /> <code>{a.path}</code></span><span className="m"><span className="cc-err">{a.error}</span></span></div>;
      const color = agentColor(a.color);
      return <div key={`${a.harness}:${a.path}`} className="pj-task ca-row" role="button" tabIndex={0} title={a.description || a.name} onClick={() => open(a)} onKeyDown={onRowKey(() => open(a))}><span className="t"><HarnessIcon agent={a.harness} className="cc-ag ca-ic" /><span className="ddot ca-dot" hidden={!color} style={color ? { background: color } : undefined} /><b>{a.name}</b> <span className="ca-desc">{a.description}</span></span><span className="m">{agentMeta(a).map((meta, index) => <span key={index}>{meta}</span>)}<code>{a.path}</code>{a.error && <span className="cc-err">{a.error}</span>}</span>{start && <span className="acts"><Button small title={`A coding session that uses ${a.name}`} onClick={e => { e.stopPropagation(); start(a); }}>Start</Button></span>}</div>;
    })}</div> : <EmptyPanel text={message} error={error} />}
  </>;
}

export function CustomAgentPrompt({ agent, close, start }: { agent: CustomAgentRow; close(): void; start: (() => void) | null }) {
  const prompt = useRef<HTMLPreElement>(null);
  useLayoutEffect(() => { prompt.current?.focus(); }, []);
  const icon = <HarnessIcon agent={agent.harness} className="cc-ag ca-ic" />;
  return <><SheetHead title={<>{icon}{agent.name}</>} /><div className="am-sub">{[agentLabel(agent.harness), ...agentMeta(agent), agent.path].join(" · ")}</div>
    {agent.description && <div className="ca-about">{agent.description}</div>}{!!agent.tools.length && <div className="am-sub">Tools: {agent.tools.join(", ")}</div>}{agent.error && <div className="am-msg bad">{agent.error}</div>}
    <pre ref={prompt} className="ca-prompt" tabIndex={0}>{agent.prompt || "No prompt."}</pre>{agent.prompt_cut && <div className="am-sub">The prompt goes on; open the file to read the rest.</div>}
    <div className="gp-foot"><Button look="quiet" onClick={close}>Close</Button>{start && <Button look="primary" onClick={start}>Start</Button>}</div></>;
}
