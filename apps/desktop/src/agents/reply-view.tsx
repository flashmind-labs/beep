import { memo } from "react";

/// Replies already contain the sanitized Markdown produced by the node.
export interface ReplyRow { row: string; markdown: boolean; html: string }

export function replyRow(html: string): ReplyRow | null {
  if (!html.startsWith('<div class="dcoderen agent')) return null;
  const template = document.createElement("template");
  template.innerHTML = html;
  const node = template.content.firstElementChild;
  if (template.content.childNodes.length !== 1 || !(node instanceof HTMLElement) || node.tagName !== "DIV"
    || !node.classList.contains("dcoderen") || !node.classList.contains("agent")) return null;
  const row = node.dataset.row;
  if (!row || !Number.isSafeInteger(Number(row)) || Number(row) < 0) return null;
  return { row, markdown: node.classList.contains("md"), html: node.innerHTML };
}

export const AgentReply = memo(function AgentReply({ reply }: { reply: ReplyRow }) {
  return <div className={`dcoderen agent${reply.markdown ? " md" : ""}`} data-row={reply.row} dangerouslySetInnerHTML={{ __html: reply.html }}/>;
});
