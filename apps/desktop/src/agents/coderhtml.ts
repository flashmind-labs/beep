// The parts a coding agent's card shares between a chat and the session
// page: its question, plan and changed files.
import { esc, ICON } from "../bar/core.ts";
import type { AgentQuestion } from "./agents.ts";

const mark = (status: string, asking: boolean) => status === "completed" ? `<span class="dok">${ICON.check}</span>`
  : status === "in_progress" ? (asking ? '<span class="ddot hot"></span>' : '<span class="dspin"></span>')
  : '<span class="ddot off"></span>';

/// The question with one button per option, marked with `attr`, the allowing
/// one primary and the denying one a warning. `keys` adds their shortcuts.
export function questionHtml(q: AgentQuestion, attr: string, busy: boolean, keys = false) {
  const allow = q.options.findIndex(x => x.allow), deny = q.options.findIndex(x => !x.allow);
  const buttons = q.options.map((o, i) => {
    const key = !keys ? "" : i === allow ? `<kbd>⌘${ICON.ret}</kbd>` : i === deny ? "<kbd>⌘D</kbd>" : "";
    return `<button class="dbtn${i === allow ? " pri" : i === deny ? " warn" : ""}" ${attr}="${i}"${busy ? " disabled" : ""}>${esc(o.name)}${key}</button>`;
  }).join("");
  return `<div class="q"><b>${esc(q.text)}</b>${q.detail ? `<code>${esc(q.detail)}</code>` : ""}<div class="dups">${buttons}</div></div>`;
}

/// The Plan section, or nothing for no plan.
export const planHtml = (plan: readonly { status: string; content: string }[], asking: boolean) => plan.length
  ? `<div class="sec">Plan</div><div class="plan">${plan.map(x => `<div class="row"><span class="dmark">${mark(x.status, asking)}</span><span${x.status === "pending" ? ' class="dmut"' : ""}>${esc(x.content)}</span></div>`).join("")}</div>`
  : "";

/// The last `max` changed files, or nothing for none.
export const filesHtml = (files: readonly string[], max: number) => files.length
  ? `<div class="sec">Files changed</div><div class="files">${files.slice(-max).map(f => `<span><b>${esc(f)}</b></span>`).join("")}${files.length > max ? `<span class="dmut">and ${files.length - max} more</span>` : ""}</div>`
  : "";
