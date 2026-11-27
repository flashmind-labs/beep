import { answerHtml } from "./turnrows.ts";
import type { AgentQuestion } from "../../stores/agents/agents-state.ts";
import { Button, Spinner } from "../../ui/kit.tsx";

/// A question on screen as its buttons show it: the key it was drawn for, whether
/// each option allows, and whether an answer is on its way.
interface ShownQuestion { key: string; allows: readonly boolean[]; disabled: boolean }
const shownQuestions = new Map<Element, ShownQuestion>();

/// The ref of the buttons' row for `shown`: ⌘⏎ and ⌘D answer what it shows.
export function questionRef(shown: ShownQuestion) {
  return (el: Element | null) => {
    if (!el) return;
    shownQuestions.set(el, shown);
    return () => { shownQuestions.delete(el); };
  };
}

/// The questions on screen, those inside `first` before the others.
export function shownQuestionsIn(first: Element | null) {
  const drawn = [...shownQuestions].filter(([el]) => el.isConnected);
  return [...drawn.filter(([el]) => first?.contains(el)), ...drawn.filter(([el]) => !first?.contains(el))].map(([, shown]) => shown);
}

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
  return <>{stopping ? <Spinner /> : <span className="dstopico" />}</>;
}
export function AgentPlan({ harness, plan, question, approvalKey, answering }: { harness: string; plan: string; question: AgentQuestion; approvalKey: string; answering: number | string | null }) {
  return <div className="ag-plan"><div className="h"><b>Plan ready</b><small>{harness} waits for you</small></div>{plan && <div className="dcoderen agent md" dangerouslySetInnerHTML={{ __html: answerHtml(plan) }} />}<div className="acts" ref={questionRef({ key: approvalKey, allows: question.options.map(option => option.allow), disabled: question.id === answering })}>{question.options.map((option, index) => <Button key={option.id} look={option.allow ? "primary" : "secondary"} data-agopt={index} data-agquestion={approvalKey} data-agallow={String(option.allow)} disabled={question.id === answering}>{option.name}</Button>)}</div></div>;
}
