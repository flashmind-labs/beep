// Markdown answers in a coding agent's session: copy controls for their
// code blocks and quotes.
import { quotedMarkdown } from "../../stores/agents/copy-markdown.ts";
import { copy, writeClipboard } from "../../ui/clipboard.ts";
import { COPY_ICON, ICON } from "../../ui/icons.tsx";

/// An answer with copy controls for code blocks and quotes.
export const answerHtml = (html: string) => html.replace(/<pre(\s[^>]*)?>/g, (_m, attrs = "") => `<pre class="dcode"${attrs}><button class="dcopy" data-copy title="Copy">Copy</button>`)
  .replace(/<\/blockquote>/g, () => `</blockquote><button type="button" class="dcopy dquote-copy" data-copy aria-label="Copy quote" title="Copy quote">${COPY_ICON}</button>`);

const BLOCKED_MESSAGE = "This request was blocked by our safety systems.";

export function conversationBlocked(error: string): boolean {
  return error.includes(BLOCKED_MESSAGE) || /\b(?:misalignment_policy_violation|cyber_policy)\b/.test(error);
}

/// Some adapters append a provider error to the last assistant chunk. Split
/// only the exact terminal notice, leaving quoted examples and code alone.
export function blockedReply(text: string, html: string): { html: string; error: string } | null {
  const at = text.lastIndexOf(BLOCKED_MESSAGE);
  if (at < 0) return null;

  const error = text.slice(at).trim();
  if (!/^This request was blocked by our safety systems\.(?:\s*Reason: [^\n]+)?$/.test(error)) return null;

  const template = document.createElement("template");
  template.innerHTML = html;
  const walker = document.createTreeWalker(template.content, NodeFilter.SHOW_TEXT);
  let joined = "";
  const nodes: Text[] = [];
  while (walker.nextNode()) {
    const node = walker.currentNode;
    if (node instanceof Text) { nodes.push(node); joined += node.data; }
  }

  const start = joined.lastIndexOf(BLOCKED_MESSAGE);
  if (start < 0) return null;

  let offset = 0;
  for (const node of nodes) {
    if (start < offset + node.length) {
      if (node.parentElement?.closest("pre, code, blockquote")) return null;

      const range = document.createRange();
      range.setStart(node, start - offset);
      range.setEnd(template.content, template.content.childNodes.length);
      range.deleteContents();
      return { html: text.slice(0, at).trim() ? template.innerHTML : "", error };
    }
    offset += node.length;
  }

  return null;
}

/// Copies the code block or quote containing the button.
export function copyCode(btn: HTMLElement) {
  const parent = btn.classList.contains("dquote-copy") ? btn.previousElementSibling : btn.parentElement;
  if (!parent) return;

  const text = parent.tagName === "BLOCKQUOTE" ? quotedMarkdown(parent) : parent.querySelector("code")?.textContent ?? "";
  const copying = btn.classList.contains("dquote-copy") ? writeClipboard(text).then(() => {
    btn.innerHTML = ICON.check;
    btn.setAttribute("aria-label", "Copied");
    btn.title = "Copied";
    setTimeout(() => {
      if (!btn.isConnected) return;

      btn.innerHTML = COPY_ICON;
      btn.setAttribute("aria-label", "Copy quote");
      btn.title = "Copy quote";
    }, 1200);
  }) : copy(text, btn);
  copying.catch(error => {
    console.warn(error);
    btn.textContent = "Copy failed";
  });
}
