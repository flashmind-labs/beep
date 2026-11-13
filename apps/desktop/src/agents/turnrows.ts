// Markdown answers in a coding agent's session: copy controls for their
// code blocks and quotes.
import { quotedMarkdown } from "./copy-markdown.ts";
import { copy, writeClipboard } from "../ui/parts.ts";
import { COPY_ICON, ICON } from "../ui/icons.tsx";

/// An answer with copy controls for code blocks and quotes.
export const answerHtml = (html: string) => html.replace(/<pre(\s[^>]*)?>/g, (_m, attrs = "") => `<pre class="dcode"${attrs}><button class="dcopy" data-copy title="Copy">Copy</button>`)
  .replace(/<\/blockquote>/g, () => `</blockquote><button type="button" class="dcopy dquote-copy" data-copy aria-label="Copy quote" title="Copy quote">${COPY_ICON}</button>`);

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
