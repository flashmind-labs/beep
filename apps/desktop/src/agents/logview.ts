// A coding agent session's log as the bar draws it: turns, tool steps,
// thoughts, subagents and questions, and where it scrolls.
import { type BeepCall, beepCall, beepLine, clock, commandDetail, commandOf, diffBlocks, duration, heading, keptSteps, keptThoughts, liveThought, onlyNotes, rowParts, runLabel, span, splitTurns, stepLabel, stepOpen, summary, thoughtLabel, toolKind, type ToolKind, toolRuns, turnStats, type Turn, type TurnState, withoutDiffs } from "./agentturns.ts";
import { $, bar, type ChatBody, esc, ICON, S } from "../bar/core.ts";
import { drawPane } from "../agentpane/media.ts";
import { collectArtifacts, toolArtifacts } from "../agentpane/artifacts-state.ts";
import { atBottom, keptScrollTop, logScroll } from "./logscroll.ts";
import { attachLive, endedHtml, liveHtml } from "./thoughtlive.ts";
import { type AgentQuestion, type AgentEntry, type AgentPage, AG, agentLog, agentPics, agentImages, AGENT_ACTIVE, agentKind, stateTag } from "./agents.ts";
import { stoppingNow, redrawAgentHints, STOP_MARK, userImages } from "./session.ts";
import { fillAgentPics, gonePic } from "./images.ts";
import { drawModelPill } from "./modelpill.ts";
import { drawQueue } from "./messages.ts";

function stepMark(status: string) {
  const running = AGENT_ACTIVE.has(AG.page?.session.status);
  return status === "completed" ? `<span class="dok" title="Done">${ICON.check}</span>`
    : status === "failed" ? `<span class="dbad" title="Failed">${ICON.x}</span>`
    : status && running ? `<span class="dspin" title="Running"></span>`
    : status ? '<span class="ddot off" title="Did not finish"></span>' : "";
}

/// A tool entry as one line: its mark and a short label ("Ran cargo test").
export function toolRow(en: AgentEntry, at: number, pics = "") {
  const log = AG.log!;
  const { title, status, output } = log.parseTool(en.text);
  if (log.subagent(output)) return subCard([at]) + (pics ? `<div class="dcoderen tool">${pics}</div>` : "");
  const failed = status === "failed", open = stepOpen(failed, at === newestStep, AG.opened.has(at));
  const live = status === "in_progress" && AGENT_ACTIVE.has(AG.page?.session.status);
  const label = stepLabel(en.text, live, open);
  const exit = failed ? log.exitCode(output) : null;
  const beep = beepCall(title);
  const long = title.includes("\n") || label.length > 80;
  // A Beep call's row shows the first line of its command or prompt; opened, all of it.
  const detail = beep ? (beep.detail.includes("\n") || beep.detail.length > 60 ? beep.detail : "") : commandDetail(title, label);
  const tail = log.tailLines(output), whole = AG.all.has(at);
  const body = !open ? ""
    : (detail ? `<pre class="cmd">${esc(detail)}</pre>` : "")
      + (output ? `<pre class="out${whole ? " all" : ""}">${esc(whole ? output : tail.shown)}</pre>` : "")
      + (tail.cut ? `<button class="more" data-all="${at}">${whole ? "Show less" : `Show all ${tail.total === 1 ? "of it" : `${tail.total} lines`}`}</button>` : "");
  const opens = !!output || long;
  const published = AG.page ? toolArtifacts(en.text, AG.page.session.cwd).files.find(f => f.published) : undefined;
  const artifact = published ? ` data-artifact="${esc(published.path)}" title="Open in the Artifacts tab"` : "";
  const state = failed ? (exit != null ? `exit ${exit}` : "Failed") : status === "completed" ? "Done" : live ? "Running" : "";
  if (beep) {
    return `<div class="dcoderen tool beep${failed ? " failed" : ""}${open ? " open" : ""}${published ? " artifact" : ""}" data-row="${at}"${opens ? ` data-at="${at}"` : ""}${artifact}>`
      + `<span class="mk">${stepMark(status)}</span>${beepHead(beep, title, state, live)}`
      + mediaButtons(title) + body + pics + "</div>";
  }
  const parts = rowParts(en.text, live);
  const rowBody = !open ? "" : (detail ? `<pre class="cmd">${esc(detail)}</pre>` : "") + toolBody(output);
  const tip = esc(commandOf(title));
  return `<div class="dcoderen tool row${failed ? " failed" : ""}${open ? " open" : ""}${published ? " artifact" : ""}" data-row="${at}"${opens ? ` data-at="${at}"` : ""}${artifact}>`
    + `<span class="ki">${KIND_ICONS[parts.kind]}</span><span class="vb">${esc(parts.verb)}</span>`
    + `<code class="tt" title="${tip}">${esc(open && long ? label : parts.target)}</code>`
    + `<span class="rm${parts.bad ? " bad" : ""}">${detailHtml(parts.detail)}</span><span class="mk">${stepMark(status)}</span>`
    + mediaButtons(title) + rowBody + pics + "</div>";
}

/// An icon for each kind of call, in the row before its verb.
const KIND_ICONS: Record<ToolKind, string> = {
  read: '<svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linejoin="round"><path d="M14 3H6v18h12V7z"/><path d="M14 3v4h4M9 13h6M9 17h4"/></svg>',
  edit: '<svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M4 20h4L19 9l-4-4L4 16z"/><path d="M13.5 6.5l4 4"/></svg>',
  command: '<svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M5 7l5 5-5 5M12 17h7"/></svg>',
  search: '<svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"><circle cx="11" cy="11" r="6"/><path d="M20 20l-4.5-4.5"/></svg>',
  web: '<svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><circle cx="12" cy="12" r="9"/><path d="M3 12h18M12 3c3 3.5 3 14.5 0 18M12 3c-3 3.5-3 14.5 0 18"/></svg>',
  mcp: '<svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M9 3v4M15 3v4M7 7h10v4a5 5 0 0 1-10 0zM12 16v5"/></svg>',
  beep: '<svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><rect x="2" y="4" width="9" height="7" rx="1.5"/><rect x="13" y="13" width="9" height="7" rx="1.5"/><path d="M6.5 11v4.5a2 2 0 0 0 2 2H13"/></svg>',
  other: '<svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M14.7 6.3a4 4 0 0 0-5.4 5.4L3 18l3 3 6.3-6.3a4 4 0 0 0 5.4-5.4l-2.5 2.5-2.4-.6-.6-2.4z"/></svg>',
};

/// The right side of a row: an edit's counts in green and red.
function detailHtml(detail: string) {
  const counts = /^\+(\d+) -(\d+)$/.exec(detail);
  return counts ? `<span class="pl">+${counts[1]}</span><span class="mi">-${counts[2]}</span>` : esc(detail);
}

/// What an opened call shows: each diff as its changed lines, then the
/// rest of its output in a mono box that scrolls.
function toolBody(output: string) {
  const diffs = diffBlocks(output).map(d => `<div class="ddiff"><div class="dh">${esc(d.path)}</div>`
    + d.lines.map(l => `<div class="${l.mark === "+" ? "da" : l.mark === "-" ? "dr" : "dc"}">${esc(l.mark + l.text) || " "}</div>`).join("")
    + (d.more ? `<div class="dm">${d.more} more lines</div>` : "") + "</div>").join("");
  const rest = diffs ? withoutDiffs(output) : output;
  return diffs + (rest ? `<pre class="out box">${esc(rest)}</pre>` : "");
}

const BEEP_ICON = '<svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><rect x="2" y="4" width="9" height="7" rx="1.5"/><rect x="13" y="13" width="9" height="7" rx="1.5"/><path d="M6.5 11v4.5a2 2 0 0 0 2 2H13"/></svg>';

/// A call to the session's Beep tools: what it did, the machine as a chip,
/// the first line of its command or prompt, then the file count and how it
/// stands on the right.
function beepHead(beep: BeepCall, title: string, state: string, live: boolean) {
  return `<span class="tt bp" title="${esc(beepLine(title) ?? title)}"><span class="bi">${BEEP_ICON}</span><span class="v">${esc(live ? beep.doing : beep.done)}</span>`
    + (beep.machine ? `<span class="mc">${esc(beep.machine)}</span>` : "")
    + (beep.detail ? `<code>${esc(beep.detail.split("\n")[0])}</code>` : "") + "</span>"
    + `<span class="bs">${esc([beep.files, state].filter(Boolean).join(" · "))}</span>`;
}

/// A button for each media file a step's title names (at most three),
/// opening it in the media pane.
function mediaButtons(title: string) {
  const paths = AG.log!.mediaPaths(title).slice(0, 3);
  return paths.length ? `<span class="dmedia">${paths.map(p => `<button data-media="${esc(p)}" title="Open ${esc(p)}">${ICON.image}${esc(p.slice(p.lastIndexOf("/") + 1))}</button>`).join("")}</span>` : "";
}

const SUB_ICON = '<svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><circle cx="6" cy="6" r="2.5"/><circle cx="18" cy="18" r="2.5"/><path d="M6 8.5V13a5 5 0 0 0 5 5h4.5"/></svg>';

/// What the window saw of each subagent: when it started and ended, by
/// session and the call's place. Calls carry no times.
const subTimes = new Map<string, { start: number; live: boolean; end?: number }>();

/// A subagent's call as its parts: name, how it stands, its steps.
function subOf(at: number) {
  const p = AG.page!, en = p.entries[at - p.earlier];
  const { title, status, output } = AG.log!.parseTool(AG.log!.imageLines(en.text).text);
  const sub = AG.log!.subagent(output) ?? { count: 0, steps: [], result: output };
  const working = (status === "in_progress" || status === "pending") && AGENT_ACTIVE.has(p.session.status);
  // Timed from when this window saw it working, if it did.
  const key = `${AG.open?.session}\n${at}`, now = Date.now();
  let seen = subTimes.get(key);
  if (!seen) subTimes.set(key, seen = { start: now, live: working });
  if (!working && seen.live && seen.end == null) seen.end = now;
  const ms = !seen.live ? null : working ? now - seen.start : (seen.end ?? now) - seen.start;
  return { title: title.split("\n")[0], agent: sub.agent ?? "", status, working, failed: status === "failed", sub, ms };
}

/// One subagent as a row: its name, how it stands, how long and how many steps.
function subRow(at: number) {
  const x = subOf(at);
  const st = x.working ? "working" : x.failed ? "failed" : "done";
  const meta = [st, x.ms != null ? duration(x.ms) : "", x.sub.count === 1 ? "1 step" : `${x.sub.count} steps`].filter(Boolean).join(" · ");
  const kind = x.agent ? `<span class="kind">${esc(x.agent)}</span>` : "";
  return `<button class="dsubrow" data-sub="${at}" title="Open ${esc(x.title)}"><span class="mk">${stepMark(x.working ? "in_progress" : x.status)}</span><span class="t"><b>${esc(x.title)}${kind}</b><small>${esc(meta)}</small></span></button>`;
}

/// Subagents started together as one card: "3 subagents · 2 working · 1 done".
function subCard(ats: number[]) {
  const all = ats.map(subOf), working = all.filter(x => x.working).length, failed = all.filter(x => x.failed).length;
  const done = all.length - working - failed;
  const counts = [working && `${working} working`, done && `${done} done`, failed && `${failed} failed`].filter(Boolean).join(" · ");
  const head = ats.length > 1 ? `<div class="h">${SUB_ICON}<b>${ats.length} subagents</b><span>${esc(counts)}</span></div>` : "";
  return `<div class="dsubgrp" data-row="${ats[0]}">${head}${ats.map(subRow).join("")}</div>`;
}

/// The places of the subagents' calls among the entries shown.
function subagentsShown(p: AgentPage) {
  if (!AG.log) return [];
  return p.entries.flatMap((en, i) => en.kind === "tool" && AG.log!.subagent(AG.log!.parseTool(en.text).output) ? [p.earlier + i] : []);
}

/// A subagent opened as its own conversation: the way back, what it was
/// asked, its steps and what it answered.
function childHtml(p: AgentPage, at: number) {
  const x = subOf(at), s = p.session, k = agentKind(s.agent);
  const steps = x.sub.steps.map(st => `<div class="dcoderen tool"><span class="mk">${stepMark(st.status)}</span><code class="tt" title="${esc(st.title)}">${esc(stepLabel(`${st.title} [${st.status}]`, st.status === "in_progress" && x.working))}</code></div>`).join("");
  const earlier = x.sub.count > x.sub.steps.length ? `<div class="dmuted">${x.sub.count - x.sub.steps.length} earlier steps not shown</div>` : "";
  const none = !x.sub.count && !x.working ? `<div class="dcoderen other">Steps not reported by ${esc(k.name)}.</div>` : "";
  const result = x.sub.result ? `<div class="dcoderen agent">${esc(x.sub.result)}</div>` : x.working ? '<div class="dmuted"><span class="dspin"></span> Working…</div>' : "";
  return `<div class="dcoderlog"><div class="dcrumb"><button id="agparent">${esc(s.title || "Session")}</button><span>›</span><b>${esc(x.title)}</b><span class="sp"></span><button class="dbtn" id="agparent2" data-parent="1">Open parent<kbd>esc</kbd></button></div>
    <div class="dcoderen user"><b>${esc(x.agent || "Task")}</b><span>${esc(x.title)}</span></div>
    <div class="dcoderturn"><span class="hd">${esc(x.working ? "Working" : x.ms != null ? `Worked for ${duration(x.ms)}` : "Worked")}</span></div>
    ${earlier}${steps}${none}${result}</div>`;
}

/// Back from a subagent to its session.
export function closeChild() {
  if (AG.child == null) return false;
  AG.child = null;
  drawAgentBody();
  return true;
}

/// An image a user entry carried: the one sent from here, the one read
/// from the session's machine by its key (an empty tile while it loads),
/// or its name when neither can be.
function entryPic(n: string) {
  const key = /^image-([0-9a-f]{16})$/.exec(n)?.[1], label = key ? "Image" : n;
  const got = key ? agentImages.get(key) : undefined;
  const url = (AG.open && agentPics.get(`${AG.open.session}\n${n}`)) || (typeof got === "string" ? got : "");
  const named = ` data-name="${esc(n)}" alt="${esc(label)}" title="${esc(label)}"`;
  if (url) return `<img class="pic" src="${esc(url)}"${named}>`;
  if (key && got !== null) return `<img class="pic" data-key="${key}"${named}>`;
  return gonePic(label);
}

/// A reply's code blocks, each with a button that copies it.
function withCopy(html: string) {
  return html.replace(/<pre(\s[^>]*)?>/g, (_m, attrs = "") => `<pre class="dcode"${attrs}><button class="dcopy" data-copy title="Copy">Copy</button>`);
}

/// The thought still arriving, by its place, which shows "Thinking"; set
/// by each draw of the turns.
let liveAt: number | null = null;
/// The running turn thinks: a thought streams or is about to.
export let thinking = false;

/// How long one pass of the "Thinking" letters takes, as in the CSS.
const SWEEP_MS = 1500;

/// "Thinking", its letters lit one after another in a loop that keeps
/// its phase across redraws, so a poll does not restart it.
const thinkingLabel = () => `<span class="lb" style="--ph:${Date.now() % SWEEP_MS}ms">${[..."Thinking"].map((c, i) => `<i style="--i:${i}">${c}</i>`).join("")}</span>`;

/// A thought as one line that opens to its text. While it streams it
/// follows its newest line; once done it folds to "Thought for 12s" unless
/// the reader opened, folded or scrolled it.
function thoughtRow(en: AgentEntry, at: number) {
  const live = at === liveAt, session = AG.open?.session ?? "";
  const m = AG.open ? turnsOf(AG.open.session) : null;
  const touched = !!m?.touched.has(at), opened = !!m?.thoughts.has(at);
  const folding = !live && !touched && m?.wasLive === at;
  const open = live ? !touched || opened : opened || folding;
  const label = live ? thinkingLabel() : `<span class="lb">${thoughtLabel(en.ms)}</span>`;
  const key = `${session}:${at}`;
  const body = live ? `<div class="tx">${liveHtml(key, en.text)}</div>`
    : folding ? `<div class="tx">${endedHtml(key, en.text)}</div>`
    : `<div class="tx md">${en.html ?? esc(en.text).replace(/\n/g, "<br>")}</div>`;
  return `<div class="dthink${live ? " live" : ""}${open ? " open" : ""}" data-row="${at}"${folding ? " data-folding" : ""}>`
    + `<button data-thought="${at}" aria-expanded="${open}">${label}<span class="cv">${ICON.chev}</span></button>`
    + `<div class="bd"><div>${body}</div></div></div>`;
}

export function entryRow(en: AgentEntry, at: number) {
  const text = esc(en.text);
  if (en.kind === "thought") return thoughtRow(en, at);
  // Codex echoes answered questions as a message: show them as answered.
  const answered = en.kind === "user" && AG.log ? AG.log.answeredQuestions(en.text) : null;
  if (answered) return `<div class="dcoderen qa" data-row="${at}">${answered.map(x => `${x.question ? `<span class="q">${esc(x.question)}</span>` : ""}<span class="a">${esc(x.answer)}</span>`).join("")}</div>`;
  if (en.kind === "user") {
    const { text: said, names } = userImages(en.text);
    const user = (s: string) => AG.log ? AG.log.userHtml(s, esc, i => AG.opened.has(`${at}:${i}`)) : esc(s);
    if (!names.length) return `<div class="dcoderen user" data-at="${at}"><b>You</b><span>${user(en.text)}</span></div>`;
    return `<div class="dcoderen user" data-at="${at}"><b>You</b>${said ? `<span>${user(said)}</span>` : ""}<span class="pics">${names.map(entryPic).join("")}</span></div>`;
  }
  // The pictures the agent or a tool showed go after the text, as a
  // user's do. The HTML is already without their lines.
  const { text: said, names } = AG.log ? AG.log.imageLines(en.text) : { text: en.text, names: [] };
  const pics = names.length ? `<span class="pics">${names.map(entryPic).join("")}</span>` : "";
  // The HTML is the desktop's own Markdown render, cleaned of anything that runs.
  // Image files the reply names link to its pictures, shown in a strip.
  const linked = (html: string) => AG.log ? AG.log.linkImagePaths(html).html : html;
  if (en.kind === "agent") return en.html ? `<div class="dcoderen agent md" data-row="${at}">${withCopy(linked(en.html))}${pics}</div>` : `<div class="dcoderen agent" data-row="${at}">${linked(esc(said))}${pics}</div>`;
  if (en.kind === "tool") return AG.log ? toolRow({ ...en, text: said }, at, pics) : `<div class="dcoderen tool">${text}</div>`;
  return `<div class="dcoderen other" data-row="${at}">${text}</div>`;
}

/// What this window knows of a session's turns: the ones it has seen, when it
/// saw a turn start or end (entries carry no times), the turn running at the
/// last draw, and the finished turns opened to their steps.
interface TurnMemory {
  known: Set<number>;
  starts: Map<number, number>;
  ends: Map<number, number>;
  running: number | null;
  opened: Set<number>;
  /// The thoughts opened to their text.
  thoughts: Set<number>;
  /// The thoughts the reader opened, folded or scrolled while they
  /// streamed, which do not fold by themselves when they end.
  touched: Set<number>;
  /// The thought streaming at the last draw.
  wasLive: number | null;
  /// The runs of calls opened, by their first call.
  runs: Set<number>;
}

const turnMemory = new Map<string, TurnMemory>();

export function turnsOf(session: string) {
  let m = turnMemory.get(session);
  if (!m) turnMemory.set(session, m = { known: new Set(), starts: new Map(), ends: new Map(), running: null, opened: new Set(), thoughts: new Set(), touched: new Set(), wasLive: null, runs: new Set() });
  return m;
}

/// Notes the turns that started or ended since the last draw. The turns
/// there at the first draw started before this window looked.
function noteTurns(m: TurnMemory, turns: Turn[], running: number | null) {
  const now = Date.now(), first = !m.known.size;
  for (const t of turns) {
    if (!m.known.has(t.key) && !first) m.starts.set(t.key, now);
    m.known.add(t.key);
  }
  if (m.running != null && m.running !== running && !m.ends.has(m.running)) m.ends.set(m.running, now);
  m.running = running;
}

/// The newest step of the running turn, which starts open when it failed.
let newestStep: number | null = null;

/// When each turn shown started, by its message's place, as the last draw
/// worked it out; for the Artifacts tab.
const turnStarts = new Map<number, number>();

/// Rows for `ats` in order: subagents started one after another as one
/// card, and finished calls of one kind in a row as one line that opens
/// to them ("Read 6 files").
function rowsOf(p: AgentPage, ats: number[]) {
  const out: string[] = [];
  let run: number[] = [];
  const isSub = (at: number) => {
    const en = p.entries[at - p.earlier];
    return en.kind === "tool" && !!AG.log?.subagent(AG.log.parseTool(en.text).output);
  };
  const flush = () => { if (run.length) out.push(subCard(run)); run = []; };
  for (const group of AG.log ? toolRuns(p.entries, ats, p.earlier) : ats.map(at => [at])) {
    if (group.length > 1) { flush(); out.push(runRow(p, group)); continue; }
    const at = group[0];
    if (isSub(at)) { run.push(at); continue; }
    flush();
    out.push(entryRow(p.entries[at - p.earlier], at));
  }
  flush();
  return out.join("");
}

/// Calls of one kind in a row as one line, the first few targets beside
/// it; it opens in place, and stays open across redraws.
function runRow(p: AgentPage, ats: number[]) {
  const en = (at: number) => p.entries[at - p.earlier];
  const kind = toolKind(en(ats[0]).text);
  const names = ats.map(at => rowParts(en(at).text).target.split("/").pop() || "").filter(Boolean);
  const shown = names.slice(0, 3).join(", ") + (names.length > 3 ? ` and ${names.length - 3} more` : "");
  const open = !!AG.open && turnsOf(AG.open.session).runs.has(ats[0]);
  return `<div class="dtrun${open ? " open" : ""}" data-row="${ats[0]}"><button data-run="${ats[0]}" aria-expanded="${open}">`
    + `<span class="ki">${KIND_ICONS[kind]}</span><span class="vb">${esc(runLabel(kind, ats.length))}</span><span class="sm">${esc(shown)}</span><span class="cv">${ICON.chev}</span></button>`
    + `<div class="bd"><div><div class="in">${ats.map(at => entryRow(en(at), at)).join("")}</div></div></div></div>`;
}

/// The log in turns: each message, then a heading with how long the agent
/// worked, then its work and its answer.
function turnsHtml(p: AgentPage) {
  const s = p.session, active = AGENT_ACTIVE.has(s.status);
  newestStep = null;
  thinking = false;
  turnStarts.clear();
  const turns = splitTurns(p.entries, p.earlier), lastTurn = turns.at(-1);
  const m = turnsOf(AG.open?.session ?? s.id);
  noteTurns(m, turns, active && lastTurn ? lastTurn.key : null);
  const row = (at: number) => entryRow(p.entries[at - p.earlier], at);
  const entry = (at: number) => p.entries[at - p.earlier];
  liveAt = lastTurn ? liveThought(p.entries, lastTurn, active, p.earlier) : null;
  const html = turns.map(t => {
    // Notes before the first message are not a turn of work.
    if (onlyNotes(p.entries, t, p.earlier)) return t.steps.map(row).join("");
    const last = t === lastTurn, running = active && last;
    const user = t.user == null ? "" : row(t.user);
    if (!running && !t.steps.length && !t.answer.length) return user;
    const { start, end } = span({
      first: p.earlier === 0 && t.key === 0, last, running,
      seenStart: m.starts.get(t.key), seenEnd: m.ends.get(t.key),
      sessionStart: s.started_at, lastActivity: s.last_activity,
    });
    if (start != null) turnStarts.set(t.key, start);
    const state: TurnState = !running ? "done" : s.status === "asking" ? "waiting" : "working";
    const ms = start == null ? null : running ? Date.now() - start : end == null ? null : end - start;
    const opened = m.opened.has(t.key);
    let work: string, stats = turnStats(p.entries, t, p.earlier);
    // The newest step of a running turn, shown as it goes.
    const newest = running ? [...t.steps].reverse().find(at => entry(at).kind === "tool") : undefined;
    if (newest != null) newestStep = newest;
    if (opened) work = rowsOf(p, t.steps);
    else {
      const kept = new Set(keptSteps(p.entries, t, p.earlier));
      if (newest != null) kept.add(newest);
      // While it runs, the thoughts since the newest step stay in view.
      if (running) for (const at of t.steps) if (entry(at).kind === "thought" && (newest == null || at > newest)) kept.add(at);
      // So do the thoughts just before it: one that ended folds to its
      // line in place instead of going into the heading.
      if (running && newest != null) {
        for (let i = t.steps.indexOf(newest) - 1; i >= 0 && entry(t.steps[i]).kind === "thought"; i--) kept.add(t.steps[i]);
      }
      keptThoughts(p.entries, t, kept, p.earlier);
      const folding = t.steps.filter(at => !kept.has(at));
      // Pictures the folded steps looked at show small under the line.
      const seen = folding.flatMap(at => entry(at).kind === "tool" && AG.log ? AG.log.imageLines(entry(at).text).names : []);
      // The heading is the fold: what the folded steps did, on its line.
      if (folding.some(at => entry(at).kind === "tool")) stats = summary(p.entries, t, p.earlier, folding);
      const pics = seen.length ? `<div class="dcoderen dfoldpics" data-row="${t.key}"><span class="pics">${seen.map(entryPic).join("")}</span></div>` : "";
      work = pics + rowsOf(p, t.steps.filter(at => kept.has(at)));
    }
    const label = `<span${running && start != null ? ` data-since="${start}" data-state="${state}"` : ""}>${esc(heading(state, ms))}</span>`
      + (stats ? `<span class="stats">· ${esc(stats)}</span>` : "");
    const head = `<div class="dcoderturn">`
      + (!t.steps.length ? `<span class="hd">${label}</span>`
        : `<button class="hd${opened ? " open" : ""}" data-turn="${t.key}" aria-expanded="${opened}" title="${opened ? "Fold the steps" : "Show the steps"}">${label}${ICON.chev}</button>`)
      + (start != null ? `<span class="when">${clock(start)}</span>` : "") + "</div>";
    // What the agent thinks or does now, on one dim line, unless the
    // newest step says it already.
    const doing = s.doing?.split("\n")[0].trim() ?? "";
    if (running) thinking = liveAt != null || doing === "Thinking";
    const shownTitle = newest != null ? AG.log?.parseTool(entry(newest).text).title.trim() : "";
    const thought = running && state === "working" && doing && doing !== shownTitle && liveAt == null
      // A thought without text yet (or one the agent keeps to itself) sweeps
      // like one that has it, with nothing to open.
      ? doing === "Thinking"
        ? `<div class="dthink live">${thinkingLabel()}</div>`
        : `<div class="dcoderthought" title="${esc(s.doing ?? "")}">${esc(doing)}</div>` : "";
    return user + head + work + thought + t.answer.map(row).join("");
  }).join("");
  m.wasLive = liveAt;
  return html;
}

let turnTick: ReturnType<typeof setInterval> | undefined;

/// Moves the running turn's clock on between polls, and stops once no
/// turn runs on screen.
function tickTurns() {
  const tick = () => {
    const els = S.view === "agent" ? bar.querySelectorAll<HTMLElement>("#agbody [data-since]") : null;
    if (!els?.length) { clearInterval(turnTick); turnTick = undefined; return; }
    for (const el of els) el.textContent = heading(el.dataset.state as TurnState, Date.now() - Number(el.dataset.since));
  };
  if (!turnTick && bar.querySelector("#agbody [data-since]")) turnTick = setInterval(tick, 1000);
}

/// The question's answers; the first that allows and the first that does
/// not take ⌘Return and ⌘D.
function questionCard(q: AgentQuestion) {
  const allow = q.options.findIndex(x => x.allow), deny = q.options.findIndex(x => !x.allow);
  const buttons = q.options.map((o, i) => `<button class="dbtn${i === allow ? " pri" : i === deny ? " warn" : ""}" data-agopt="${i}"${q.id === AG.answering ? " disabled" : ""}>${esc(o.name)}${i === allow ? `<kbd>⌘${ICON.ret}</kbd>` : i === deny ? "<kbd>⌘D</kbd>" : ""}</button>`);
  return `<div class="q"><b style="font-weight:600">${esc(q.text)}</b>${q.detail ? `<code style="font-family:var(--mono);font-size:12px;white-space:pre-wrap;overflow-wrap:anywhere">${esc(q.detail)}</code>` : ""}
      <div class="dups">${buttons.join("")}</div></div>`;
}

/// A row of the log a redraw puts back where it was: found by `sel`, `off`
/// pixels from the log's top.
interface ScrollAnchor {
  sel: string | null;
  off: number;
  scroll: number;
}

/// The row a click opens or folds, as what finds it again once the log
/// is drawn again: a turn by its heading, a step by its entry.
export function clickedRow(el: HTMLElement) {
  const turn = el.closest<HTMLElement>("[data-turn]");
  if (turn) return `[data-turn="${turn.dataset.turn}"]`;
  const at = el.closest<HTMLElement>("[data-at]");
  return at ? `[data-at="${at.dataset.at}"]` : undefined;
}

/// Where the reader is in the log: the row `sel` when given and there,
/// else the first row in view, and how far it sits from the top, so a
/// redraw puts it back there.
function scrollAnchor(b: HTMLElement, sel?: string): ScrollAnchor {
  const top = b.getBoundingClientRect().top;
  const picked = sel ? b.querySelector<HTMLElement>(sel) : null;
  if (sel && picked) return { sel, off: picked.getBoundingClientRect().top - top, scroll: b.scrollTop };
  for (const el of b.querySelectorAll<HTMLElement>("[data-row]")) {
    const r = el.getBoundingClientRect();
    if (r.bottom > top) return { sel: `[data-row="${el.dataset.row}"]`, off: r.top - top, scroll: b.scrollTop };
  }
  return { sel: null, off: 0, scroll: b.scrollTop };
}

function restoreScroll(b: HTMLElement, a: ScrollAnchor) {
  const el = a.sel != null ? b.querySelector<HTMLElement>(a.sel) : null;
  if (el) b.scrollTop = keptScrollTop(b.scrollTop, a.off, el.getBoundingClientRect().top - b.getBoundingClientRect().top);
  else b.scrollTop = a.scroll;
}

/// Draws the log again. `keep` finds the row the reader just opened or
/// folded, which stays where it is on screen.
export function drawAgentBody(keep?: string) {
  const b = $<ChatBody>("#agbody"), p = AG.page;
  if (!b || !p) return;
  const s = p.session, q = s.question;
  const st = $("#agstate"), stopping = stoppingNow();
  if (st) st.innerHTML = stopping ? '<span class="dstate run">Stopping</span>' : stateTag(s.status);
  if (!AG.log) agentLog().then(() => { if (S.view === "agent") drawAgentBody(); }).catch(console.warn);
  // Read where the reader is before the log is drawn again: only a reader
  // at the bottom follows new entries; anyone else stays where they are,
  // and so does a row the reader opens or folds.
  const scroll = logScroll(b.stuck !== false, keep != null);
  const anchor = scroll === "keep" ? scrollAnchor(b, keep) : null;
  const child = AG.child != null && AG.log && p.entries[AG.child - p.earlier] ? AG.child : null;
  if (child == null) AG.child = null;
  const mark = (x: string) => x === "completed" ? `<span class="dok">${ICON.check}</span>` : x === "in_progress" ? (s.status === "asking" ? '<span class="ddot" style="background:var(--hot)"></span>' : '<span class="dspin"></span>') : '<span class="ddot off"></span>';
  const question = q ? questionCard(q) : "";
  const plan = p.plan.length ? `<div class="sec">Plan</div><div class="plan">${p.plan.map(x => `<div class="row"><span style="width:16px;display:grid;place-items:center">${mark(x.status)}</span><span style="${x.status === "pending" ? "color:var(--muted)" : ""}">${esc(x.content)}</span></div>`).join("")}</div>` : "";
  const files = p.files.length ? `<div class="sec">Files changed</div><div class="files">${p.files.slice(-12).map(f => `<span><b>${esc(f)}</b></span>`).join("")}${p.files.length > 12 ? `<span style="color:var(--muted)">and ${p.files.length - 12} more</span>` : ""}</div>` : "";
  // Steers sent from here show above the field; this counts the others.
  const others = p.queued - AG.queue.filter(q => q.state === "sent" || q.state === "sending").length;
  const queued = others > 0 ? `<div class="row" style="padding:8px 12px 0;color:var(--muted)">${others === 1 ? "1 message waits" : `${others} messages wait`} for the turn to end</div>` : "";
  // The question, plan and files sit under the log, by the field.
  const card = question || plan || files || queued
    ? `<div class="dcard dcoder">${question}${plan}${files}${queued}<div style="height:12px"></div></div>` : "";
  const why = AG.why ? `<div class="dmuted">Auto picked ${esc(s.machine_name)}. ${esc(AG.why)}</div>` : "";
  const earlier = p.earlier ? `<div class="dmuted">${p.earlier} earlier ${p.earlier === 1 ? "entry" : "entries"} not shown</div>` : "";
  const entries = child != null ? childHtml(p, child) : p.entries.length && AG.log ? `<div class="dcoderlog">${earlier}${turnsHtml(p)}</div>`
    : s.status === "starting" ? '<div class="dmuted"><span class="dspin"></span> Starting the agent…</div>' : "";
  const err = p.error ? `<div class="derrcard"><b>The turn did not finish</b><span>${esc(p.error)}</span></div>` : "";
  const note = AG.note === "gone" ? '<div class="dmuted">This session is no longer on the machine, or the machine went offline.</div>'
    : AG.note && AG.note === AG.readError && AG.note.startsWith("Reconnecting") ? `<div class="dmuted">${esc(AG.note)}</div>`
    : AG.note ? `<div class="derr">${esc(AG.note)}</div>` : "";
  const paused = AG.paused ? '<button class="dbtn quiet" id="agresume">Paused, click to refresh</button>' : "";
  const subs = child == null ? subagentsShown(p) : [];
  b.innerHTML = `<div class="dagmain">${why}${entries}${child == null ? card : ""}${err}${note}${paused}</div>`
    + `<button class="dagjump" id="agjump" hidden>Jump to latest${DOWN_ICON}</button>`;
  const arts = child == null ? { list: collectArtifacts(p.entries, p.earlier, s.cwd), turnStart: (k: number) => turnStarts.get(k) } : undefined;
  drawPane({ html: subs.map(subRow).join(""), count: subs.length }, arts);
  fillAgentPics(b);
  attachLive(b, key => { if (AG.open) turnsOf(AG.open.session).touched.add(Number(key.split(":").pop())); });
  drawModelPill();
  drawQueue();
  tickTurns();
  redrawAgentHints();
  // An ended session goes on from the field: the next message resumes it.
  const msg = $<HTMLTextAreaElement>("#agmsg"), k = agentKind(s.agent);
  if (msg) msg.placeholder = s.status === "ended" ? `Message ${k.name} to continue` : `Message ${k.name}`;
  // Only a turn that runs can be stopped.
  const stop = $<HTMLButtonElement>("#agstop");
  if (stop) {
    stop.style.display = AGENT_ACTIVE.has(s.status) ? "" : "none";
    // Stopped: it says so at once, while the agent winds the turn down.
    if (stop.disabled !== stopping) {
      stop.disabled = stopping;
      stop.innerHTML = stopping ? '<span class="dspin"></span>Stopping' : `${STOP_MARK}Stop`;
    }
  }
  if (scroll === "bottom") b.scrollTop = b.scrollHeight;
  else if (anchor) restoreScroll(b, anchor);
  // A row opened at the bottom pushes the bottom away: the log follows
  // new entries again once the reader scrolls back down.
  if (keep != null) b.stuck = atBottom(b);
  syncJump(b);
}

const DOWN_ICON = '<svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round"><path d="M12 5v14M6 13l6 6 6-6"/></svg>';

/// Shows "Jump to latest" while the log is scrolled up from the bottom.
export function syncJump(b: ChatBody) {
  const jump = b.querySelector<HTMLElement>("#agjump");
  if (jump) jump.hidden = b.stuck !== false || atBottom(b);
}

/// Scrolls the log to its newest entry, which it follows again from there.
export function jumpToLatest() {
  const b = $<ChatBody>("#agbody");
  if (!b) return;
  b.stuck = true;
  b.scrollTo({ top: b.scrollHeight, behavior: matchMedia("(prefers-reduced-motion: reduce)").matches ? "auto" : "smooth" });
  syncJump(b);
}
