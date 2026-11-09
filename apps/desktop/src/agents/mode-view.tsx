import { answerHtml } from "./turnrows.ts";
import type { AgentQuestion } from "./agents.ts";

export function AgentMode({ name, plan }: { name: string; plan: boolean }) {
  return <button className={`agc-pill agmode${plan ? " on" : ""}`} id="agmodeb" title="Switch mode (Shift+Tab)">{name}</button>;
}
export function AgentPlan({ harness, plan, question, answering }: { harness: string; plan: string; question: AgentQuestion; answering: number | string | null }) {
  return <div className="ag-plan"><div className="h"><b>Plan ready</b><small>{harness} waits for you</small></div>{plan && <div className="dcoderen agent md" dangerouslySetInnerHTML={{ __html: answerHtml(plan) }} />}<div className="acts">{question.options.map((option, index) => <button key={option.id} className={`dbtn${option.allow ? " pri" : ""}`} data-agopt={index} disabled={question.id === answering}>{option.name}</button>)}</div></div>;
}
