// The said rows of a coding agent's log: the user's prompt, the agent's
// reply and what came with a prompt (Beep's context, another session's
// message, a child's reply). Their bodies are sanitized Markdown HTML.
import { conversationBlocked } from "./turnrows.ts";
import { memo, useMemo, useRef, type MouseEvent } from "react";
import { domToReact, type DOMNode } from "html-react-parser";
import { OUTCOME_LABEL, REPORT_MIN_WORDS, reportOf, reportViews, textOf, toggleOriginal, type Report, type ReportSection } from "./report-state.ts";
import { DraftNotes, SentReviewCard, draftsIn, useDrafts } from "./review-view.tsx";
import type { ReviewComment, SentReview } from "./review-state.ts";
import { useStore } from "zustand";
import { plural } from "../ui/format.ts";
import { HarnessIcon } from "../ui/icons.tsx";
import { Button, CopyButton, Spinner, useHtml } from "../ui/kit.tsx";
import { sessionMessageTitle, type PromptPart } from "./promptparts-state.ts";
import { EntryTime } from "./turn-heading-view.tsx";
import { SubIcon } from "./activity-view.tsx";

/// Replies already contain the sanitized Markdown produced by the node.
/// `text` is the reply as the agent wrote it, which its copy button copies.
/// `final` marks the last reply of a finished turn, which may be a report;
/// `idle` that the session takes a message now, as a report's Summarize sends;
/// `summary` the shorter version Summarize brought, drawn in its place.
export interface ReplyRow { kind: "reply"; row: string; markdown: boolean; html: string; text: string; copy: boolean; final: boolean; idle: boolean; summary: Summary | null; sentAt?: number | null }

/// A report's summary: being written by the session's agent, or its reply,
/// whose `id` keeps the reader's choice between it and the original.
export type Summary = { state: "writing" } | { state: "done"; id: string; html: string; text: string };
export interface BlockedRow { kind: "blocked"; id: string; message: string }
/// `review` is the prompt read as a review Beep sent, drawn as its comments.
export interface PromptRow { kind: "prompt"; row: string; html: string; files: string[]; pictures: string; review: SentReview | null; sentAt?: number | null }
export type ContextRow = { kind: "context"; id: string; type: "context"; text: string } | { kind: "context"; id: string; type: "session"; name: string; project: string | null; html: string } | { kind: "context"; id: string; type: "child"; part: Extract<PromptPart, { kind: "child" }>; body: string; agent: string | null; name: string; project: string | null };

/// A turn's last reply that reads as a report is drawn as one.
export const AgentReply = memo(function AgentReply({ reply }: { reply: ReplyRow }) {
  const report = useMemo(() => reply.final && reply.markdown ? reportOf(reply.html, reply.text) : null, [reply.final, reply.markdown, reply.html, reply.text]);

  return report ? <ReportReply reply={reply} report={report} /> : <PlainReply reply={reply} />;
});

/// Reconciled rather than set as HTML, so a streamed chunk leaves finished blocks, and their diagrams, in place.
function PlainReply({ reply }: { reply: ReplyRow }) {
  const body = useHtml(reply.html), drafts = useDrafts(reply.row);
  return <div className={`dcoderen agent${reply.markdown ? " md" : ""}`} data-row={reply.row}><EntryTime at={reply.sentAt ?? null} />{reply.copy && <CopyButton text={reply.text} look="quiet" small icon className="reply-copy" />}<div className="reply-body">{body}<DraftNotes drafts={drafts} /></div></div>;
}

/// A report: its outcome, length and reading time over its headline and
/// parts, its prose set at a reading measure. Once summarized, the summary
/// takes its place, and the header's button turns between the two.
function ReportReply({ reply, report }: { reply: ReplyRow; report: Report }) {
  const summary = reply.summary, drafts = useDrafts(reply.row);
  const done = summary?.state === "done" ? summary : null;
  const original = useStore(reportViews, views => done !== null && views.original.has(done.id));
  const short = useMemo(() => done && reportOf(done.html, done.text, 0), [done?.html, done?.text]);
  const shown = short && !original ? short : report;
  const view = shown === report ? "report" : "summary";
  // The text fades in when the view changes here, not when the card is drawn again.
  const drawn = useRef(view);
  const swapped = drawn.current !== view;
  drawn.current = view;
  const outcome = shown.kind === "shaped" ? shown.shaped.outcome : null;

  return <div className="dcoderen agent md report" data-row={reply.row} data-view={view}>
    <EntryTime at={reply.sentAt ?? null} />
    {reply.copy && <CopyButton text={shown === report ? reply.text : done?.text ?? reply.text} look="quiet" small icon className="reply-copy" />}
    <div className="report-head">
      {outcome && <span className="report-outcome" data-outcome={outcome}>{OUTCOME_LABEL[outcome]}</span>}
      <span className="report-label">{shown === report ? "Report" : "Summary"}</span>
      <span className="report-length">{`${plural(shown.words, "word", "words")}, about ${shown.minutes} min`}</span>
      <span className="report-acts">
        {summary?.state === "writing" ? <span className="report-busy"><Spinner />Summarizing…</span>
          : done ? <Button small look="quiet" onClick={() => toggleOriginal(done.id)} title={original ? "Show the summary" : "Show the full report"}>{original ? "Summary" : "Original"}</Button>
          : reply.idle && report.words >= REPORT_MIN_WORDS && <Button small look="quiet" data-report-summarize={reply.row} title="Ask the agent for a shorter version of this report">Summarize</Button>}
      </span>
    </div>
    <ReportBody key={view} report={shown} swapped={swapped} drafts={drafts} />
  </div>;
}

/// A report's text: its headline, opening and parts.
/// Each draft sits under the part it quotes: the headline and opening, or a section; the rest at the end.
function ReportBody({ report, swapped, drafts }: { report: Report; swapped: boolean; drafts: readonly ReviewComment[] }) {
  const parts = report.kind === "long" ? [] : [[...report.shaped.headline, ...report.shaped.intro], ...report.shaped.sections.map(section => section.nodes)];
  const placed = parts.map(part => draftsIn(drafts, part.map(textOf).join(" ")));
  const elsewhere = drafts.filter(c => !placed.some(list => list.includes(c)));

  return <div className={`reply-body${swapped ? " swapped" : ""}`}>
    {report.kind === "long" ? nodes(report.nodes) : <>
      {report.shaped.headline.length > 0 && <p className="report-headline">{nodes(report.shaped.headline)}</p>}
      {nodes(report.shaped.intro)}
      <DraftNotes drafts={placed[0]} />
      {report.shaped.sections.map((section, i) => <ReportPart key={i} section={section} drafts={placed[i + 1].filter(c => !placed.slice(0, i + 1).some(list => list.includes(c)))} />)}
    </>}
    <DraftNotes drafts={elsewhere} />
  </div>;
}

/// A report's `##` section: its title and item count over its body; Needs you as a callout.
function ReportPart({ section, drafts }: { section: ReportSection; drafts: readonly ReviewComment[] }) {
  return <section className="report-part" data-kind={section.kind}>
    <h2>{section.title}{section.count > 0 && <span className="n">{section.count}</span>}</h2>
    {nodes(section.nodes)}
    <DraftNotes drafts={drafts} />
  </section>;
}

const nodes = (list: DOMNode[]) => list.length ? domToReact(list) : null;

/// `switchModel` offers to run the failed prompt again on a model the user picks.
export function AgentError({ message, switchModel = false }: { message: string; switchModel?: boolean }) {
  return <div className="derrcard" role="alert"><b>{conversationBlocked(message) ? "Conversation blocked" : "The turn did not finish"}</b><span>{message}</span>{switchModel && <Button small data-agretrymodel="">Retry with another model</Button>}</div>;
}

/// A triple-click selects the whole prompt: macOS WebKit's own paragraph
/// selection runs on past it to the end of the log.
export const AgentPrompt = memo(function AgentPrompt({ prompt }: { prompt: PromptRow }) {
  const said = useRef<HTMLDivElement>(null);
  const selectPrompt = (event: MouseEvent) => {
    if (event.detail < 3 || !said.current) return;

    event.preventDefault();
    getSelection()?.selectAllChildren(said.current);
  };

  if (prompt.review) return <div className="dcoderen user review" data-at={prompt.row}><b>You <EntryTime at={prompt.sentAt ?? null} /></b><SentReviewCard review={prompt.review} /></div>;
  return <div className="dcoderen user md" data-at={prompt.row} onMouseDown={selectPrompt}><b>You <EntryTime at={prompt.sentAt ?? null} /></b>{prompt.html && <div className="said" ref={said} dangerouslySetInnerHTML={{ __html: prompt.html }} />}{prompt.files.length > 0 && <AttachedFiles paths={prompt.files} />}{prompt.pictures && <span className="pics" dangerouslySetInnerHTML={{ __html: prompt.pictures }} />}</div>;
});

/// The files a prompt named, by file name. More than five fold under a
/// count. A click opens the file (`session-click.ts`).
function AttachedFiles({ paths }: { paths: string[] }) {
  const items = <div className="attachment-items">{paths.map((path, i) => <button type="button" key={i} className="attachment-file" data-attachment-path={path} title={path}>{path.split(/[\\/]/).filter(Boolean).pop() ?? path}</button>)}</div>;

  return paths.length > 5 ? <details className="attachment-batch"><summary>{paths.length} attachments</summary>{items}</details> : items;
}

export const AgentContext = memo(function AgentContext({ row }: { row: ContextRow }) {
  if (row.type === "context") return <details className="dcoderen beepctx"><summary>Beep context</summary><pre>{row.text}</pre></details>;
  if (row.type === "child") {
    const title = sessionMessageTitle(row.name) || "Child session";
    const state = row.part.state === "asking" ? "Waiting on you" : row.part.state === "closed" ? "Session ended" : row.part.state === "stopped" ? "Stopped" : "Done";
    // The header opens the child's session; its reply stays selectable.
    return <div className="dcoderen kidreply sessioncard" role="group" aria-label={`Reply from ${title}`}><button className="kh" type="button" data-kid={row.part.id} title={`Open ${title}`}>{row.agent ? <HarnessIcon agent={row.agent} className="dapp dcoderav" /> : <SubIcon />}<span className="kt"><span className="kl">{row.project || "Session reply"}</span><b>{title}</b></span><span className="ks" data-state={row.part.state}>{state}</span></button><div className="dcoderen agent md sessionbody" dangerouslySetInnerHTML={{ __html: row.body }} /></div>;
  }
  const title = sessionMessageTitle(row.name) || "Other session";
  return <div className="dcoderen fromsession sessioncard" role="group" aria-label={`Message from ${title}`}><div className="kh"><SubIcon /><span className="kt"><span className="kl">{row.project ? `From ${row.project}` : "Message from session"}</span><b>{title}</b></span></div><div className="sessionbody" dangerouslySetInnerHTML={{ __html: row.html }} /></div>;
});
