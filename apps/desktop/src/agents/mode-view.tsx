import { answerHtml } from "./turnrows.ts";
import type { AgentQuestion } from "./agents.ts";

export function AgentMode({ name, plan }: { name: string; plan: boolean }) {
  return <button className={`agc-pill agmode${plan ? " on" : ""}`} id="agmodeb" title="Switch mode (Shift+Tab)">{name}</button>;
}
export function AgentStatus({ status, stopping }: { status: string; stopping: boolean }) {
  if (stopping) return <span className="dstate run">Stopping</span>;
  if (status === "asking") return <span className="dstate ask">Waiting for you</span>;
  if (status === "waiting") return <span className="dstate ask">Waiting in terminal</span>;
  if (status === "working" || status === "running" || status === "starting") return <span className="dstate run">{status === "starting" ? "Starting" : "Working"}</span>;
  return <span className="dstate">{status === "ended" ? "Ended" : "Idle"}</span>;
}
export function AgentStopFace({ stopping }: { stopping: boolean }) {
  return <><span className={stopping ? "dspin" : "dstopico"} />{stopping ? "Stopping" : "Stop"}</>;
}
export function AgentPlan({ harness, plan, question, approvalKey, answering }: { harness: string; plan: string; question: AgentQuestion; approvalKey: string; answering: number | string | null }) {
  return <div className="ag-plan"><div className="h"><b>Plan ready</b><small>{harness} waits for you</small></div>{plan && <div className="dcoderen agent md" dangerouslySetInnerHTML={{ __html: answerHtml(plan) }} />}<div className="acts">{question.options.map((option, index) => <button key={option.id} className={`dbtn${option.allow ? " pri" : ""}`} data-agopt={index} data-agquestion={approvalKey} data-agallow={String(option.allow)} disabled={question.id === answering}>{option.name}</button>)}</div></div>;
}
