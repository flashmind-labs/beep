import { useEffect, useRef, useState } from "react";
import { agentKind, type AgentPage, type AgentSession } from "../agents/agents.ts";
import { loadSessions } from "../agents/sessions.ts";
import { openAgent } from "../agents/session.ts";
import { HarnessIcon, ICON } from "../ui/icons.tsx";
import { subscribeChanges, topic } from "../ui/mesh-events.ts";
import { paneShown } from "../ui/active.ts";
import { invoke } from "../ui/tauri.ts";
import { useHeld } from "./work-view.tsx";
import { cardKey, type AgentReport } from "./agentcard.ts";

// ---------- Coding agent card in a reply ----------

// A `code_agent` call shows the session it started or checked as a card
// under the reply's steps: its state, plan, files and any question, live
// while it works, with the question's buttons and Open. It follows its
// session while its chat stays cached, reading only while it shows, and
// reads again each time its chat is put back on screen (`shown`).

interface Found { row: AgentSession; page: AgentPage | null }

export function CoderCard({ report, shown }: { report: AgentReport; shown: number }) {
  const box = useRef<HTMLDivElement>(null), reads = useRef(0);
  const [found, setFound] = useState<Found | null>(null), [answering, setAnswering] = useState<number | null>(null);
  const row = found?.row ?? null;

  const poll = async (row: AgentSession) => {
    if (!box.current?.isConnected) return;
    const version = ++reads.current;
    const page = await invoke<AgentPage>("agent_view", { machineId: row.machine_id, agent: row.agent, session: row.id, last: 1 });
    if (reads.current !== version || !box.current?.isConnected) return;
    setFound(current => current?.row === row ? { row, page } : current);
    setAnswering(current => page.session.question?.id === current ? current : null);
  };
  const read = (row: AgentSession) => void poll(row).catch(error => console.warn("agent card:", error));

  useEffect(() => {
    let active = true;
    void loadSessions().catch((): AgentSession[] => []).then(rows => {
      const row = rows.find(s => s.id === report.id && s.agent === report.agent);
      if (active && row && box.current?.isConnected) setFound({ row, page: null });
    });
    return () => { active = false; };
  }, [report.id, report.agent]);

  useEffect(() => {
    if (!row) return;
    read(row);
    const unfollow = subscribeChanges(topic("agent", row.id), { deliver: "latest", receive: () => poll(row) }, { machine: row.machine_id, when: () => !!box.current?.isConnected && paneShown() });
    return () => { unfollow(); reads.current++; };
  }, [row]);

  const first = useRef(true);
  useEffect(() => {
    if (first.current) { first.current = false; return; }
    if (row) read(row);
  }, [shown]);

  const answer = async (index: number) => {
    const question = found?.page?.session.question, option = question?.options[index];
    if (!found || !question || !option || answering === question.id) return;
    setAnswering(question.id);

    try {
      await invoke("center_answer_agent", { machineId: found.row.machine_id, agent: found.row.agent, session: found.row.id, question: question.id, reply: option.id });
    } catch (e) {
      // A conflict means it was answered already, so it stays answered.
      if (!/^conflict/i.test(String(e))) setAnswering(null);
      console.warn("answering the agent:", e);
    }

    read(found.row);
  };

  const value = useHeld<CoderCardState>({ report, row, page: found?.page ?? null, answering }, box, sameCard);
  return <div className="dcard dcoder dcodercard" data-card={cardKey(report)} ref={box}><CoderCardView value={value} answer={index => void answer(index)} open={() => { if (found) openAgent(found.page?.session ?? found.row); }} /></div>;
}

const sameCard = (a: CoderCardState, b: CoderCardState) => a.report === b.report && a.row === b.row && a.page === b.page && a.answering === b.answering;

export interface CoderCardState { report: AgentReport; row: AgentSession | null; page: AgentPage | null; answering: number | null }
function CoderCardView({ value, answer, open }: { value: CoderCardState; answer(index: number): void; open(): void }) {
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
