import { memo } from "react";
import { esc } from "../ui/format.ts";

/// Replies already contain the sanitized Markdown produced by the node.
export interface ReplyRow { kind: "reply"; row: string; markdown: boolean; html: string }
export interface PromptRow { kind: "prompt"; row: string; html: string; pictures: string }
export type ContextRow = { kind: "context"; id: string; type: "context"; text: string } | { kind: "context"; id: string; type: "session"; name: string; html: string };
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
  return `<div class="dcoderen fromsession"><b>Other session wrote${row.name ? `<span class="who">${esc(row.name)}</span>` : ""}</b><span>${row.html}</span></div>`;
}

export const AgentContext = memo(function AgentContext({ row }: { row: ContextRow }) {
  if (row.type === "context") return <details className="dcoderen beepctx"><summary>Beep context</summary><pre>{row.text}</pre></details>;
  return <div className="dcoderen fromsession"><b>Other session wrote{row.name && <span className="who">{row.name}</span>}</b><span dangerouslySetInnerHTML={{ __html: row.html }} /></div>;
});
