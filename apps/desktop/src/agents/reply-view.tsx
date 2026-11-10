import { memo } from "react";
import { esc } from "../ui/format.ts";
import { HarnessIcon } from "../ui/icons.tsx";
import { childReplyHtml } from "./childreply-state.ts";
import type { PromptPart } from "./promptparts-state.ts";

/// Replies already contain the sanitized Markdown produced by the node.
export interface ReplyRow { kind: "reply"; row: string; markdown: boolean; html: string }
export interface PromptRow { kind: "prompt"; row: string; html: string; pictures: string }
export type ContextRow = { kind: "context"; id: string; type: "context"; text: string } | { kind: "context"; id: string; type: "session"; name: string; html: string } | { kind: "context"; id: string; type: "child"; part: Extract<PromptPart, { kind: "child" }>; body: string; icon: string; agent: string | null; meta: string };
export function replyHtml(reply: ReplyRow) {
  return `<div class="dcoderen agent${reply.markdown ? " md" : ""}" data-row="${reply.row}">${reply.html}</div>`;
}

export function replyRow(html: string): ReplyRow | null {
  if (!html.startsWith('<div class="dcoderen agent')) return null;
  const template = document.createElement("template");
  template.innerHTML = html;
  const node = template.content.firstElementChild;
  if (template.content.childNodes.length !== 1 || !(node instanceof HTMLElement) || node.tagName !== "DIV"
    || !node.classList.contains("dcoderen") || !node.classList.contains("agent")) return null;
  const row = node.dataset.row;
  if (!row || !Number.isSafeInteger(Number(row)) || Number(row) < 0) return null;
  return { kind: "reply", row, markdown: node.classList.contains("md"), html: node.innerHTML };
}

export const AgentReply = memo(function AgentReply({ reply }: { reply: ReplyRow }) {
  return <div className={`dcoderen agent${reply.markdown ? " md" : ""}`} data-row={reply.row} dangerouslySetInnerHTML={{ __html: reply.html }}/>;
});

export function promptHtml(prompt: PromptRow) {
  return `<div class="dcoderen user md" data-at="${prompt.row}"><b>You</b>${prompt.html ? `<div class="said">${prompt.html}</div>` : ""}${prompt.pictures ? `<span class="pics">${prompt.pictures}</span>` : ""}</div>`;
}

export const AgentPrompt = memo(function AgentPrompt({ prompt }: { prompt: PromptRow }) {
  return <div className="dcoderen user md" data-at={prompt.row}><b>You</b>{prompt.html && <div className="said" dangerouslySetInnerHTML={{ __html: prompt.html }} />}{prompt.pictures && <span className="pics" dangerouslySetInnerHTML={{ __html: prompt.pictures }} />}</div>;
});

export function contextHtml(row: ContextRow) {
  if (row.type === "context") return `<details class="dcoderen beepctx"><summary>Beep context</summary><pre>${esc(row.text)}</pre></details>`;
  if (row.type === "child") return childReplyHtml(row.part, row.body, row.icon, row.meta);
  return `<div class="dcoderen fromsession"><b>Other session wrote${row.name ? `<span class="who">${esc(row.name)}</span>` : ""}</b><span>${row.html}</span></div>`;
}

export const AgentContext = memo(function AgentContext({ row }: { row: ContextRow }) {
  if (row.type === "context") return <details className="dcoderen beepctx"><summary>Beep context</summary><pre>{row.text}</pre></details>;
  if (row.type === "child") {
    const title = row.part.name || "Child agent";
    const state = row.part.state === "asking" ? "Waiting on you" : row.part.state === "closed" ? "Session ended" : row.part.state === "stopped" ? "Stopped" : "Done";
    const agent = row.agent;
    return <div className="dcoderen kidreply" role="group" aria-label={`Reply from ${title}`}><button className="kh" type="button" data-kid={row.part.id} title={`Open ${title}`}>{agent ? <HarnessIcon agent={agent} className="dapp dcoderav" /> : <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round"><circle cx="6" cy="6" r="2.5" /><circle cx="18" cy="18" r="2.5" /><path d="M6 8.5V13a5 5 0 0 0 5 5h4.5" /></svg>}<b>{title}</b><span className="m">{row.meta}</span><span className="ks">{state}</span></button><div className="dcoderen agent md" dangerouslySetInnerHTML={{ __html: row.body }} /></div>;
  }
  return <div className="dcoderen fromsession"><b>Other session wrote{row.name && <span className="who">{row.name}</span>}</b><span dangerouslySetInnerHTML={{ __html: row.html }} /></div>;
});
