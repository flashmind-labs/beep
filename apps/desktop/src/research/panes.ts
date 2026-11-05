// An opened research as HTML: its head, the lead's chat, and the side pane's
// tabs, each as one string the page patches into place (open.ts).
import { answerHtml, userRowHtml } from "../agents/turnrows.ts";
import { esc } from "../ui/format.ts";
import { ICON } from "../ui/icons.ts";
import { chipHtml, phaseStep, PHASES, STATUS_LABEL } from "./research-list.ts";
import { type Agent, type Entry, type Note, type Open, type Said, RUNNING, agentOf, count, doing, earlier, facts, host, kind, look, progress, round, shownTab } from "./research-state.ts";
import type { Doing } from "./research-state.ts";

const rav = (id: string) => { const l = look(id); return `<span class="rav" style="--c:${l.c}">${l.ini}</span>`; };
const ok = () => `<span class="dok">${ICON.check}</span>`;
const stateIcon = (st: Doing[1]) => st === "run" ? '<span class="dspin"></span>'
  : st === "ok" ? ok()
  : st === "wait" ? '<span class="ddot hot"></span>' : "<span></span>";
const DOWN = '<svg width="10" height="10" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="3" stroke-linecap="round" stroke-linejoin="round"><path d="M6 9l6 6 6-6"/></svg>';

/// The Export PDF button, which shows the export while it runs.
function exportButton(o: Open) {
  return o.exporting
    ? '<button class="dbtn" disabled><span class="dspin"></span>Exporting</button>'
    : '<button class="dbtn" data-act="export">Export PDF</button>';
}

// ---------- Head ----------

/// The question, where the research is, the facts, and Stop while the team works.
export function headHtml(o: Open) {
  const r = o.research, st = r?.status;
  const step = st ? phaseStep(st) : 0;
  const steps = step ? `<span class="cc-rsteps">${PHASES.map((_, i) => `<i class="${i + 1 < step ? "done" : i + 1 === step ? "now" : ""}"></i>`).join("")}</span>` : "";
  const chip = st ? chipHtml(st, STATUS_LABEL[st]) : o.sending !== null ? chipHtml("researching", "Planning") : "";
  const line = facts(o);
  return `<span class="rdocic">${ICON.doc}</span><span class="tt"><b>${esc(r?.topic || "Research")}</b>${line ? `<small>${esc(line)}</small>` : ""}</span>${chip}${steps}`
    + (RUNNING.has(st) ? '<button class="dbtn small warn" data-act="stop">Stop</button>' : "");
}

// ---------- The lead's chat ----------

/// The chat as the session's log: your messages, the lead's replies, and
/// while the team works a line of where it is.
export function chatHtml(o: Open) {
  const st = o.research?.status;
  const last = o.turns.findLastIndex(t => t.role === "assistant");
  let html = "";
  o.turns.forEach((t, i) => {
    if (t.role === "user") html += userRowHtml(esc(t.text));
    else if (t.role === "scheduled") html += `<div class="dmuted">${esc(t.label || "Research finished")}</div>`;
    else if (t.role === "assistant" && t.html) {
      html += `<div class="dcoderen agent md">${answerHtml(t.html)}</div>`;
      if (i === last && st === "planning" && o.sending === null) html += `<div class="racts"><button class="dbtn pri" data-act="go">Start<kbd>⌘${ICON.ret}</kbd></button></div>`;
      if (i === last && st === "done" && o.sending === null) html += `<div class="racts"><button class="dbtn" data-act="tab:draft">Open the paper</button>${exportButton(o)}</div>`;
    }
  });
  if (o.sending !== null) html += `<div class="dcoderen agent md">${o.sending}<span class="caret"></span></div>`;
  if (RUNNING.has(st)) html += `<div class="rstatus"><span class="dspin"></span>${esc(progress(o, Date.now()))}</div>`;
  if (o.error) html += `<div class="derr">${esc(o.error)}</div>`;
  return html;
}

/// The message field's placeholder.
export function placeholder(o: Open) {
  const st = o.research?.status;
  return st === "planning" ? "Say go, or what to change" : st === "done" ? "Ask a follow-up" : "Message the lead";
}

// ---------- Side pane ----------

/// The tab strip: the workspace, the team, then the threads and the room opened.
export function tabsHtml(o: Open) {
  const on = shownTab(o);
  const notes = o.entries.filter(e => e.kind === "note").length;
  const tab = (id: string, label: string, n = 0, close = false) =>
    `<button class="agtab${on === id ? " on" : ""}" role="tab" aria-selected="${on === id}" data-act="tab:${esc(id)}" title="${esc(label)}"><span>${esc(label)}</span>${n ? `<small>${n}</small>` : ""}`
    + (close ? `<em role="button" data-act="close:${esc(id)}" aria-label="Close ${esc(label)}" title="Close">${ICON.x}</em>` : "") + "</button>";
  return tab("draft", "Draft") + tab("notes", "Notes", notes) + tab("sources", "Sources", o.research?.sources.length) + tab("agents", "Agents")
    + o.tabs.map(id => id === "room" ? tab(id, "Room", 0, true) : tab(id, agentOf(o, id).name, 0, true)).join("");
}

/// The tab on screen, with what sits under the strip.
export function paneHtml(o: Open) {
  const tab = shownTab(o);
  if (tab === "draft") return draftHtml(o);
  if (tab === "notes") return notesHtml(o);
  if (tab === "sources") return sourcesHtml(o);
  if (tab === "room") return roomHtml(o);
  if (tab === "agents") return agentsHtml(o);
  return threadHtml(o, agentOf(o, tab));
}

const empty = (text: string) => `<div class="rbox rempty">${text}</div>`;

/// Citation numbers as links to their sources, which open in the browser.
function sup(o: Open, numbers: number[]) {
  const links = numbers.map(n => {
    const s = o.research?.sources.find(s => s.number === n);
    return s ? `<a class="rcite" href="${esc(s.url)}" title="${esc(s.title)}">${n}</a>` : String(n);
  });
  return `<sup>${links.join(", ")}</sup>`;
}

/// Turns "[3]" and "[1, 4]" in rendered text into citation marks.
function cite(o: Open, html: string) {
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

/// The paper, with its Copy and Export PDF below.
function draftHtml(o: Open) {
  const r = o.research, st = r?.status ?? "planning";
  if (!r || (st === "planning" && !o.entries.some(e => e.kind === "note"))) return `<div class="rpane">${empty("The workspace fills in once the team starts.")}</div>`;
  if (!r.draft) return `<div class="rpane">${empty(RUNNING.has(st) ? "The writer starts after the critic's review." : "No paper was written.")}</div>`;
  const by = `Written by ${esc(agentOf(o, "writer").name)}, reviewed by ${esc(agentOf(o, "critic").name)}. ${r.sources.length} sources.`;
  return `<div class="rpane"><div class="rdoc"><span class="by">${by}</span>${cite(o, r.draft_html)}</div></div>`
    + `<div class="agpacts"><span class="meta">Markdown, ${count(r.sources.length, "source")}</span><span class="sp"></span><button class="dbtn" data-act="copy">Copy</button>${exportButton(o)}</div>`;
}

/// A note, headed `title`, with its citations.
function noteHtml(o: Open, n: Entry & Note, title: string) {
  const src = n.sources.length ? ` ${sup(o, n.sources)}` : "";
  return `<div class="rmsg">${rav(n.agent)}<b>${title}</b><p>${esc(n.content)}${src}</p></div>`;
}

function notesHtml(o: Open) {
  const notes = o.entries.filter((e): e is Entry & Note => e.kind === "note");
  return `<div class="rpane">${notes.length ? `<div class="rbox">${notes.map(n => noteHtml(o, n, esc(agentOf(o, n.agent).name))).join("")}</div>` : empty("No notes yet.")}</div>`;
}

function sourcesHtml(o: Open) {
  const sources = o.research?.sources ?? [];
  if (!sources.length) return `<div class="rpane">${empty("No sources yet.")}</div>`;
  return `<div class="rpane"><div class="rtbl" role="table"><span class="th">#</span><span class="th">Source</span><span class="th">Kept by</span>${sources.map(s =>
    `<span class="n">${s.number}</span><span class="src"><a href="${esc(s.url)}">${esc(s.title)}</a><small>${esc(host(s.url))}</small><q>${esc(s.quote)}</q></span><span class="by">${rav(s.agent)}${esc(agentOf(o, s.agent).name)}</span>`).join("")}</div></div>`;
}

function msgHtml(o: Open, e: Entry & Said) {
  const to = e.to ? ` <span class="to">to ${esc(agentOf(o, e.to).name)}</span>` : "";
  return `<div class="rmsg">${rav(e.agent)}<b>${esc(agentOf(o, e.agent).name)}${to}</b><p>${esc(e.content)}</p></div>`;
}

/// The agents of the round picked, the room's button and the round menu.
function agentsHtml(o: Open) {
  const r = o.research;
  const row = (a: Agent) => {
    const [now, st] = doing(o, a.id);
    return `<button class="rag" data-act="agent:${esc(a.id)}">${rav(a.id)}<b>${esc(a.name)}</b>${stateIcon(st)}<small>${esc(now || a.role)}</small><span class="model">${esc(a.model)}</span></button>`;
  };
  // One round's scouts at a time, picked in the header; the lead, critic and writer work in every round.
  const agents = r?.agents ?? [{ id: "lead", name: "Lead", role: "Plans and coordinates", model: "", round: 0 }];
  const rounds = round(o), pick = Math.min(o.pick ?? rounds, rounds);
  const scoutOf = (a: Agent) => kind(a.id) === "scout" ? Math.max(a.round ?? 0, 1) : 0;
  const rows = [
    ...agents.filter(a => a.id === "lead"),
    ...agents.filter(a => scoutOf(a) === pick),
    ...agents.filter(a => a.id === "critic" || a.id === "writer"),
  ].map(row).join("");
  const scouts = (n: number) => count(agents.filter(a => scoutOf(a) === n).length, "scout");
  const menu = o.menu ? `<span class="dmenu" role="menu">${Array.from({ length: rounds }, (_, i) => rounds - i).map(n =>
    `<button class="mi" role="menuitemradio" aria-checked="${n === pick}" data-act="round:${n}"><b>Round ${n}</b>${n === pick ? `<span class="ok">${ICON.check}</span>` : ""}<small>${scouts(n)}</small></button>`).join("")}</span>` : "";
  const picker = rounds > 1 ? `<div class="rhd"><h4>Agents</h4><span class="rround"><button class="dbtn small" data-act="rounds" aria-haspopup="menu" aria-expanded="${o.menu}">Round ${pick}${DOWN}</button>${menu}</span></div>` : "";
  const said = o.entries.filter(e => e.kind === "said").length;
  const room = r && r.status !== "planning" ? count(said, "message") : "Opens when the team starts";
  return `${picker}<div class="rpane agents"><div class="rbox">${rows}</div>
      <div class="rbox"><button class="rag" data-act="room"><span class="rav" style="--c:var(--muted)">#</span><b>Room</b><span></span><small>${room}</small></button></div></div>`;
}

/// One agent's steps, notes and messages, and what it does now.
function threadHtml(o: Open, a: Agent) {
  const mine = o.entries.filter(e => e.agent === a.id || (e.kind === "said" && e.to === a.id));
  let html = "", steps: string[] = [];
  const flush = () => { if (steps.length) html += `<div class="dsteps">${steps.join("")}</div>`; steps = []; };
  for (const e of mine) {
    if (e.kind === "step" && e.agent === a.id) {
      steps.push(`<div class="dstep"><span class="s">${e.success ? ok() : `<span class="dbad">${ICON.x}</span>`}</span><span>${esc(e.label)}</span></div>`);
    } else if (e.kind === "said") { flush(); html += msgHtml(o, e); }
    else if (e.kind === "note") { flush(); html += noteHtml(o, e, "Note"); }
  }
  flush();
  const [now, st] = doing(o, a.id);
  if (st === "run") html += `<div class="dsteps"><div class="dstep run"><span class="s"><span class="dspin"></span></span><span>${esc(now)}</span></div></div>`;
  return `<div class="rhd">${rav(a.id)}<span class="who"><b>${esc(a.name)}</b><small>${esc(a.role)} · ${esc(a.model)}</small></span></div>`
    + `<div class="rpane thread">${html || '<div class="dmuted">Nothing yet.</div>'}</div>`;
}

function roomHtml(o: Open) {
  const said = o.entries.filter((e): e is Entry & Said => e.kind === "said");
  return `<div class="rhd"><b>Room</b><small>${count(said.length, "message")}</small></div>`
    + `<div class="rpane thread">${said.map(e => msgHtml(o, e)).join("") || '<div class="dmuted">Nobody has said anything yet.</div>'}</div>`;
}

/// Whether a tab's view follows the newest line, as a thread or the room does.
export const followsEnd = (tab: string) => tab !== "draft" && tab !== "notes" && tab !== "sources" && tab !== "agents";
