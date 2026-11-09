import { useLayoutEffect, useRef } from "react";
import { agentLabel } from "./projects-state.ts";
import { harnessInner, harnessMark } from "../ui/harnessicon.ts";
import { agentColor, agentMeta, type CustomAgentRow } from "./customagents-state.ts";

export function CustomAgentsView({ rows, message, error, reload, open, start }: { rows: CustomAgentRow[] | null; message: string; error: boolean; reload: (() => void) | null; open: (row: CustomAgentRow) => void; start: (row: CustomAgentRow) => void }) {
  return <><div className="cc-sec"><h4>Custom agents{!!rows?.length && <span className="n">{rows.length}</span>}</h4>{reload && <button type="button" className="dbtn quiet small" onClick={reload}>Reload</button>}</div>
    {rows?.length ? <div className="cc-panel pv-scroll">{rows.map(a => {
      if (a.error && !a.description && !a.prompt) return <div key={`${a.harness}:${a.path}`} className="pj-task ca-row bad"><span className="t"><span className="ddot bad" /> <code>{a.path}</code></span><span className="m"><span className="cc-err">{a.error}</span></span></div>;
      const color = agentColor(a.color), safe = /^[a-z0-9-]+$/.test(a.harness) ? ` ${a.harness}` : "", mark = harnessMark(a.harness);
      return <div key={`${a.harness}:${a.path}`} className="pj-task ca-row" role="button" tabIndex={0} title={a.description || a.name} onClick={() => open(a)} onKeyDown={e => { if (e.target === e.currentTarget && (e.key === "Enter" || e.key === " ")) { e.preventDefault(); open(a); } }}><span className="t"><span className={`cc-ag ca-ic${safe}${mark ? " mark" : ""}`} dangerouslySetInnerHTML={{ __html: harnessInner(a.harness) }} /><span className="ddot ca-dot" hidden={!color} style={color ? { background: color } : undefined} /><b>{a.name}</b> <span className="ca-desc">{a.description}</span></span><span className="m">{agentMeta(a).map((meta, index) => <span key={index}>{meta}</span>)}<code>{a.path}</code>{a.error && <span className="cc-err">{a.error}</span>}</span><span className="acts"><button type="button" className="dbtn small" title={`A coding session that uses ${a.name}`} onClick={e => { e.stopPropagation(); start(a); }}>Start</button></span></div>;
    })}</div> : <div className="cc-panel cc-empty"><span className={error ? "cc-err" : ""}>{message}</span></div>}
  </>;
}

export function CustomAgentPrompt({ agent, close, start }: { agent: CustomAgentRow; close(): void; start(): void }) {
  const prompt = useRef<HTMLPreElement>(null);
  useLayoutEffect(() => { prompt.current?.focus(); }, []);
  const safe = /^[a-z0-9-]+$/.test(agent.harness) ? ` ${agent.harness}` : "";
  const icon = <span className={`cc-ag ca-ic${safe}${harnessMark(agent.harness) ? " mark" : ""}`} dangerouslySetInnerHTML={{ __html: harnessInner(agent.harness) }} />;
  return <><div className="am-head"><b>{icon}{agent.name}</b><kbd>esc</kbd></div><div className="am-sub">{[agentLabel(agent.harness), ...agentMeta(agent), agent.path].join(" · ")}</div>
    {agent.description && <div className="ca-about">{agent.description}</div>}{!!agent.tools.length && <div className="am-sub">Tools: {agent.tools.join(", ")}</div>}{agent.error && <div className="am-msg bad">{agent.error}</div>}
    <pre ref={prompt} className="ca-prompt" tabIndex={0}>{agent.prompt || "No prompt."}</pre>{agent.prompt_cut && <div className="am-sub">The prompt goes on; open the file to read the rest.</div>}
    <div className="gp-foot"><button type="button" className="dbtn quiet" onClick={close}>Close</button><button type="button" className="dbtn pri" onClick={start}>Start</button></div></>;
}
