// The said rows of a coding agent's log: the user's prompt, the agent's
// reply and what came with a prompt (Beep's context, another session's
// message, a child's reply). Their bodies are sanitized Markdown HTML.
import { memo } from "react";
import { HarnessIcon } from "../ui/icons.tsx";
import type { PromptPart } from "./promptparts-state.ts";
import { SubIcon } from "./activity-view.tsx";

/// Replies already contain the sanitized Markdown produced by the node.
export interface ReplyRow { kind: "reply"; row: string; markdown: boolean; html: string }
export interface PromptRow { kind: "prompt"; row: string; html: string; pictures: string }
export type ContextRow = { kind: "context"; id: string; type: "context"; text: string } | { kind: "context"; id: string; type: "session"; name: string; html: string } | { kind: "context"; id: string; type: "child"; part: Extract<PromptPart, { kind: "child" }>; body: string; agent: string | null; meta: string };

export const AgentReply = memo(function AgentReply({ reply }: { reply: ReplyRow }) {
  return <div className={`dcoderen agent${reply.markdown ? " md" : ""}`} data-row={reply.row} dangerouslySetInnerHTML={{ __html: reply.html }}/>;
});

export const AgentPrompt = memo(function AgentPrompt({ prompt }: { prompt: PromptRow }) {
  return <div className="dcoderen user md" data-at={prompt.row}><b>You</b>{prompt.html && <div className="said" dangerouslySetInnerHTML={{ __html: prompt.html }} />}{prompt.pictures && <span className="pics" dangerouslySetInnerHTML={{ __html: prompt.pictures }} />}</div>;
});

export const AgentContext = memo(function AgentContext({ row }: { row: ContextRow }) {
  if (row.type === "context") return <details className="dcoderen beepctx"><summary>Beep context</summary><pre>{row.text}</pre></details>;
  if (row.type === "child") {
    const title = row.part.name || "Child agent";
    const state = row.part.state === "asking" ? "Waiting on you" : row.part.state === "closed" ? "Session ended" : row.part.state === "stopped" ? "Stopped" : "Done";
    // The header opens the child's session; its reply stays selectable.
    return <div className="dcoderen kidreply" role="group" aria-label={`Reply from ${title}`}><button className="kh" type="button" data-kid={row.part.id} title={`Open ${title}`}>{row.agent ? <HarnessIcon agent={row.agent} className="dapp dcoderav" /> : <SubIcon />}<b>{title}</b><span className="m">{row.meta}</span><span className="ks">{state}</span></button><div className="dcoderen agent md" dangerouslySetInnerHTML={{ __html: row.body }} /></div>;
  }
  return <div className="dcoderen fromsession"><b>Other session wrote{row.name && <span className="who">{row.name}</span>}</b><span dangerouslySetInnerHTML={{ __html: row.html }} /></div>;
});
