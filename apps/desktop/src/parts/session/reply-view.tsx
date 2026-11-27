// The said rows of a coding agent's log: the user's prompt, the agent's
// reply and what came with a prompt (Beep's context, another session's
// message, a child's reply). Their bodies are sanitized Markdown HTML.
import { conversationBlocked } from "./turnrows.ts";
import { memo, type MouseEvent } from "react";
import { HarnessIcon } from "../../ui/icons.tsx";
import { Button, CopyButton, useHtml } from "../../ui/kit.tsx";
import { sessionMessageTitle, type PromptPart } from "../../stores/agents/promptparts-state.ts";
import { EntryTime } from "./turn-heading-view.tsx";
import { SubIcon } from "./activity-view.tsx";

/// Replies already contain the sanitized Markdown produced by the node.
/// `text` is the reply as the agent wrote it, which its copy button copies.
export interface ReplyRow { kind: "reply"; row: string; markdown: boolean; html: string; text: string; copy: boolean; sentAt?: number | null }
export interface BlockedRow { kind: "blocked"; id: string; message: string }
export interface PromptRow { kind: "prompt"; row: string; html: string; files: string[]; pictures: string; sentAt?: number | null }
export type ContextRow = { kind: "context"; id: string; type: "context"; text: string } | { kind: "context"; id: string; type: "session"; name: string; project: string | null; html: string } | { kind: "context"; id: string; type: "child"; part: Extract<PromptPart, { kind: "child" }>; body: string; agent: string | null; name: string; project: string | null };

/// Reconciled rather than set as HTML, so a streamed chunk leaves finished blocks, and their diagrams, in place.
export const AgentReply = memo(function AgentReply({ reply }: { reply: ReplyRow }) {
  const body = useHtml(reply.html);
  return <div className={`dcoderen agent${reply.markdown ? " md" : ""}`} data-row={reply.row}><EntryTime at={reply.sentAt ?? null} />{reply.copy && <CopyButton text={reply.text} look="quiet" small icon className="reply-copy" />}<div className="reply-body">{body}</div></div>;
});

/// `switchModel` offers to run the failed prompt again on a model the user picks.
export function AgentError({ message, switchModel = false }: { message: string; switchModel?: boolean }) {
  return <div className="derrcard" role="alert"><b>{conversationBlocked(message) ? "Conversation blocked" : "The turn did not finish"}</b><span>{message}</span>{switchModel && <Button small data-agretrymodel="">Retry with another model</Button>}</div>;
}

/// A triple-click selects the whole prompt: macOS WebKit's own paragraph
/// selection runs on past it to the end of the log.
export const AgentPrompt = memo(function AgentPrompt({ prompt }: { prompt: PromptRow }) {
  const selectPrompt = (event: MouseEvent) => {
    if (event.detail < 3 || event.button !== 0 || (event.target instanceof Element && event.target.closest("summary, button, [role=button]"))) return;

    event.preventDefault();
    getSelection()?.selectAllChildren(event.currentTarget);
  };

  return <div className="dcoderen user md" data-at={prompt.row}><b>You <EntryTime at={prompt.sentAt ?? null} /></b>{prompt.html && <div className="said" onMouseDown={selectPrompt} dangerouslySetInnerHTML={{ __html: prompt.html }} />}{prompt.files.length > 0 && <AttachedFiles paths={prompt.files} />}{prompt.pictures && <span className="pics" dangerouslySetInnerHTML={{ __html: prompt.pictures }} />}</div>;
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
