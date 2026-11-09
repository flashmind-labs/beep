// An opened research as HTML: its head, the lead's chat, and the side pane's
// tabs, each as one string the page patches into place (open.ts).
import { esc } from "../ui/format.ts";
import { type Open } from "./research-state.ts";

/// The message field's placeholder.
export function placeholder(o: Open) {
  const st = o.research?.status;
  return st === "planning" ? "Say go, or what to change" : st === "done" ? "Ask a follow-up" : "Message the lead";
}

// ---------- Side pane ----------

/// Citation numbers as links to their sources, which open in the browser.
function sup(o: Open, numbers: number[]) {
  const links = numbers.map(n => {
    const s = o.research?.sources.find(s => s.number === n);
    return s ? `<a class="rcite" href="${esc(s.url)}" title="${esc(s.title)}">${n}</a>` : String(n);
  });
  return `<sup>${links.join(", ")}</sup>`;
}

/// Turns "[3]" and "[1, 4]" in rendered text into citation marks.
export function cite(o: Open, html: string) {
  const t = document.createElement("template");
  t.innerHTML = html;
  const walk = document.createTreeWalker(t.content, NodeFilter.SHOW_TEXT);
  const texts: Text[] = [];
  while (walk.nextNode()) if (!walk.currentNode.parentElement?.closest("a, code, pre")) texts.push(walk.currentNode as Text);

  for (const node of texts) {
    if (!/\[\d+(?:\s*,\s*\d+)*\]/.test(node.data)) continue;
    const span = document.createElement("span");
    span.innerHTML = esc(node.data).replace(/\[(\d+(?:\s*,\s*\d+)*)\]/g, (_, list: string) => sup(o, list.split(",").map(Number)));
    node.replaceWith(...span.childNodes);
  }

  return t.innerHTML;
}

/// Whether a tab's view follows the newest line, as a thread or the room does.
export const followsEnd = (tab: string) => tab !== "draft" && tab !== "notes" && tab !== "sources" && tab !== "agents";
