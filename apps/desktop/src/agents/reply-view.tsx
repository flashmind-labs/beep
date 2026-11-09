import { memo } from "react";

/// Replies already contain the sanitized Markdown produced by the node.
export interface ReplyRow { kind: "reply"; row: string; markdown: boolean; html: string }
export interface PromptRow { kind: "prompt"; row: string; html: string; pictures: string }
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
