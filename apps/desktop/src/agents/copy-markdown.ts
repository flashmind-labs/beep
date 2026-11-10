import TurndownService from "turndown";
import { gfm } from "turndown-plugin-gfm";

const markdown = new TurndownService({ headingStyle: "atx", codeBlockStyle: "fenced" });
markdown.use(gfm);

/// The markdown of a rendered quote, without its copy buttons. Turndown reads
/// a detached copy, so the page is left as it is.
export function quotedMarkdown(quote: Element): string {
  const container = document.createElement("div");
  container.append(...[...quote.childNodes].map(node => node.cloneNode(true)));
  container.querySelectorAll("button").forEach(button => button.remove());
  return markdown.turndown(container);
}

function output(node: Node | null): Element | null {
  const el = node instanceof Element ? node : node?.parentElement;
  return el?.closest(".dcoderen.agent.md") ?? null;
}

document.addEventListener("copy", event => {
  const selection = window.getSelection();
  if (!selection || selection.isCollapsed || !event.clipboardData) return;
  const start = output(selection.anchorNode), end = output(selection.focusNode);
  if (!start || start !== end) return;
  const fragment = selection.getRangeAt(0).cloneContents();
  fragment.querySelectorAll("button").forEach(button => button.remove());
  const container = document.createElement("div");
  container.append(fragment);
  const text = markdown.turndown(container);
  if (!text) return;
  event.clipboardData.setData("text/plain", text);
  event.preventDefault();
});
