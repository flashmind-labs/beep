import { agentKind, type AgentPage, type AgentSession } from "../agents/agents.ts";
import { HarnessIcon, ICON } from "../ui/icons.tsx";
import type { AgentReport } from "./agentcard.ts";

export interface CoderCardState { report: AgentReport; row: AgentSession | null; page: AgentPage | null; answering: number | null }
export function CoderCardView({ value, answer, open }: { value: CoderCardState; answer(index: number): void; open(): void }) {
  const { report: r, row, page } = value, session = page?.session, status = session?.status ?? r.status, asking = status === "asking";
  const plan = page ? page.plan : r.plan, files = page ? page.files : r.files, doing = session ? session.doing : r.doing, machine = session?.machine_name ?? row?.machine_name;
  const question = session?.question;
  const tag = status === "asking" ? { cls: "ask", label: "Waiting for you" } : ["working", "running", "starting"].includes(status) ? { cls: "run", label: status === "starting" ? "Starting" : "Working" } : status === "waiting" ? { cls: "ask", label: "Waiting in terminal" } : { cls: "", label: status === "ended" ? "Ended" : "Idle" };
  const allow = question?.options.findIndex(o => o.allow) ?? -1, deny = question?.options.findIndex(o => !o.allow) ?? -1;
  return <><div className="head"><HarnessIcon agent={r.agent} className="dapp dcoderav" /><div className="dcol"><b>{agentKind(r.agent).name}{machine ? ` on ${machine}` : ""}</b><small className="dmono dell" title={r.cwd}>{r.cwd}</small></div>{status !== "unknown" && <span className={`dstate${tag.cls ? ` ${tag.cls}` : ""}`}>{tag.label}</span>}</div>
    {doing && status !== "ended" && status !== "idle" && <div className="row note dell" title={doing}>{doing.split("\n")[0]}</div>}
    {question ? <div className="q"><b>{question.text}</b>{question.detail && <code>{question.detail}</code>}<div className="dups">{question.options.map((option, i) => <button type="button" key={option.id} className={`dbtn${i === allow ? " pri" : i === deny ? " warn" : ""}`} data-cardopt={i} disabled={value.answering === question.id} onClick={e => { e.stopPropagation(); answer(i); }}>{option.name}</button>)}</div></div> : !row && r.question && asking ? <div className="q"><b>{r.question}</b></div> : null}
    {plan.length > 0 && <><div className="sec">Plan</div><div className="plan">{plan.map((step, i) => <div className="row" key={i}><span className="dmark">{step.status === "completed" ? <span className="dok" dangerouslySetInnerHTML={{ __html: ICON.check }} /> : step.status === "in_progress" ? asking ? <span className="ddot hot" /> : <span className="dspin" /> : <span className="ddot off" />}</span><span className={step.status === "pending" ? "dmut" : undefined}>{step.content}</span></div>)}</div></>}
    {files.length > 0 && <><div className="sec">Files changed</div><div className="files">{files.slice(-8).map((file, i) => <span key={i}><b>{file}</b></span>)}{files.length > 8 && <span className="dmut">and {files.length - 8} more</span>}</div></>}
    {row && <div className="dups"><button type="button" className="dbtn" data-cardopen onClick={e => { e.stopPropagation(); open(); }}>Open</button></div>}
  </>;
}
