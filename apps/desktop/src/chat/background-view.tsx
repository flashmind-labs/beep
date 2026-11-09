import { Fragment } from "react";
import { AppLogo } from "./applogo-view.tsx";
import { forRoutine, type Decision } from "./chat.ts";
import { answerWorkById, type HeldQuestion } from "../bar/work.ts";
import { ICON } from "../ui/icons.ts";

const SAID = { allow: "Allowed once. The work goes on.", session: "Allowed for the rest of this chat. The work goes on.", always: "Allowed, and always allowed from now on. The work goes on.", deny: "Denied. The work goes on without it.", grant: "Allowed for this routine. The work goes on." };
const BUTTONS: readonly [Decision, string, string, string][] = [["allow", "Allow", "pri", `⌘${ICON.ret}`], ["session", "Allow for this session", "", `⇧⌘${ICON.ret}`], ["always", "Always", "", `⌥⌘${ICON.ret}`], ["deny", "Deny", "warn", "⌘D"]];

export interface BackgroundQuestions { session: string | null; questions: HeldQuestion[]; first: string | null; chatApproval: boolean }
export function BackgroundView({ value }: { value: BackgroundQuestions }) {
  return <><div className="dmuted">{value.first ? "Background work is waiting for you" : "Background work"}</div>{value.questions.map(question => question.withdrawn || question.answer
    ? <div className="dmuted" key={question.query_id}>{question.withdrawn ? `${question.action} with ${question.app}: no longer waiting` : SAID[forRoutine(question.action) && question.answer === "allow" ? "grant" : question.answer ?? "deny"]}</div>
    : <BackgroundCard key={question.query_id} question={question} session={value.session} shortcut={question.query_id === value.first && !value.chatApproval} />)}</>;
}

function BackgroundCard({ question, session, shortcut }: { question: HeldQuestion; session: string | null; shortcut: boolean }) {
  return <div className="dcard" data-q={question.query_id}><div className="head"><AppLogo app={question.app} /><div className="dcol"><b>{question.action}?</b><small>Background work asks first</small></div></div>{!!question.fields.length && <div className="kv">{question.fields.map((field, index) => <Fragment key={index}><span>{field.label}</span><span>{field.value}</span></Fragment>)}</div>}<div className="acts">{BUTTONS.filter(([decision]) => (decision !== "always" && decision !== "session") || !forRoutine(question.action)).map(([decision, label, cls, key]) => <button className={`dbtn ${cls}`} data-d={decision} key={decision} onClick={() => void answerWorkById(session, question.query_id, decision)}>{label}{shortcut && <kbd dangerouslySetInnerHTML={{ __html: key }} />}</button>)}</div></div>;
}
