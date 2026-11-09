import { chipClass, phaseStep, PHASES, STATUS_LABEL, type ResearchStatus } from "./research-list.ts";
import { facts, RUNNING, type Entry, type Research } from "./research-state.ts";

interface Props {
  research: Research | null;
  entries: readonly Entry[];
  sending: string | null;
}

export function ResearchHead({ research, entries, sending }: Props) {
  const status = research?.status;
  const step = status ? phaseStep(status) : 0;
  const line = facts({ research, entries });
  return <>
    <span className="rdocic"><svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinejoin="round"><path d="M6 2.5h8l4.5 4.5v14.5H6z"/><path d="M14 2.5V7h4.5"/></svg></span>
    <span className="tt"><b>{research?.topic || "Research"}</b>{line && <small>{line}</small>}</span>
    {status ? <Status status={status} label={STATUS_LABEL[status]}/> : sending !== null && <Status status="researching" label="Planning"/>}
    {step > 0 && <span className="cc-rsteps">{PHASES.map((phase, index) => <i key={phase} className={index + 1 < step ? "done" : index + 1 === step ? "now" : ""}/>)}</span>}
    {RUNNING.has(status) && <button className="dbtn small warn" data-act="stop">Stop</button>}
  </>;
}

function Status({ status, label }: { status: ResearchStatus; label: string }) {
  const cls = chipClass(status);
  const spinning = status === "researching" || status === "reviewing" || status === "writing";
  return <span className={`chip${cls ? ` ${cls}` : ""}`}>{spinning && <span className="dspin"/>}{label}</span>;
}
