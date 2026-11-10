import type { AgentQuestion } from "./agents.ts";
import { ICON } from "../ui/icons.tsx";

export interface LogShell {
  why: string | null;
  starting: boolean;
  log: boolean;
  question: AgentQuestion | null;
  questionKey: string | null;
  answering: boolean;
  plan: readonly { status: string; content: string }[];
  asking: boolean;
  error: string | null;
  external: boolean;
  note: { text: string; error: boolean } | null;
  retry: boolean;
}

export function AgentLogShell({ value, logRef }: { value: LogShell; logRef: (node: HTMLDivElement | null) => void }) {
  const question = value.question;
  const allow = question?.options.findIndex(option => option.allow) ?? -1;
  const deny = question?.options.findIndex(option => !option.allow) ?? -1;
  return <><div className="dagmain">
    {value.why && <div className="dmuted" data-log-part="why">{value.why}</div>}
    {value.log ? <div className="dcoderlog" ref={logRef} /> : value.starting && <div className="dmuted" data-log-part="starting"><span className="dspin" /> Starting the agent…</div>}
    {(question || value.plan.length > 0) && <div className="dcard dcoder" data-log-part="card">
      {question && <div className="q"><b>{question.text}</b>{question.detail && <code>{question.detail}</code>}<div className="dups">{question.options.map((option, index) => <button key={index} className={`dbtn${index === allow ? " pri" : index === deny ? " warn" : ""}`} data-agopt={index} data-agquestion={value.questionKey ?? ""} data-agallow={String(option.allow)} disabled={value.answering}>{option.name}{index === allow ? <kbd>⌘<svg width="11" height="11" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.6" strokeLinecap="round" strokeLinejoin="round"><path d="M20 5v7a3 3 0 0 1-3 3H5m4-4l-4 4 4 4" /></svg></kbd> : index === deny ? <kbd>⌘D</kbd> : null}</button>)}</div></div>}
      {value.plan.length > 0 && <><div className="sec">Plan</div><div className="plan">{value.plan.map((item, index) => <div className="row" key={index}><span className="dmark">{item.status === "completed" ? <span className="dok" dangerouslySetInnerHTML={{ __html: ICON.check }} /> : item.status === "in_progress" ? value.asking ? <span className="ddot hot" /> : <span className="dspin" /> : <span className="ddot off" />}</span><span className={item.status === "pending" ? "dmut" : undefined}>{item.content}</span></div>)}</div></>}
    </div>}
    {value.error && (value.external ? <div className="derr" data-log-part="error">{value.error}</div> : <div className="derrcard" data-log-part="error"><b>The turn did not finish</b><span>{value.error}</span></div>)}
    {value.note && <div className={value.note.error ? "derr" : "dmuted"} data-log-part="note">{value.note.text}</div>}
    {value.retry && <button className="dbtn quiet" id="agresume" data-log-part="retry">Retry</button>}
  </div><button className="dagjump" id="agjump" hidden>Jump to latest<svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.2" strokeLinecap="round" strokeLinejoin="round"><path d="M12 5v14M6 13l6 6 6-6" /></svg></button></>;
}
