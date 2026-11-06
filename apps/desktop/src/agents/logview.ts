// A coding agent session's log as the bar draws it: turns, tool steps,
// thoughts, subagents and questions, and where it scrolls.
import { esc, linkified, money, tokenCount } from "../ui/format.ts";
import { type BeepCall, beepCall, beepLine, commandOf, diffBlocks, duration, keptSteps, keptThoughts, liveThought, onlyNotes, rowParts, runLabel, span, splitTurns, stepLabel, stepOpen, summary, toolKind, toolRuns, turnStats, type Turn, type TurnState, withoutDiffs } from "./agentturns.ts";
import { cachedCommandHtml } from "./command-state.ts";
import { $, S } from "../bar/core.ts";
import { drawPane } from "../agentpane/media.ts";
import { followAgent } from "../agentpane/worktree.ts";
import { collectArtifacts, madeNote, toolArtifacts } from "../agentpane/artifacts-state.ts";
import { atBottom, keptScrollTop, logScroll } from "./logscroll.ts";
import { attachLive } from "./thoughtlive.ts";
import { patchable, patchRows, startPatching } from "./logpatch.ts";
import { fillLog, type Unit } from "./logwindow.ts";
import { answerHtml, errorCardHtml, KIND_ICONS, liveLineHtml, stepMark, stopFace, thoughtHtml, tickTurns, toolRowHtml, turnHeadHtml, userRowHtml } from "./turnrows.ts";
import { type AgentQuestion, type AgentEntry, type AgentPage, AG, agentLog, agentPics, agentThumbs, AGENT_ACTIVE, agentKind, stateTag } from "./agents.ts";
import { stoppingNow, redrawAgentHints, userImages } from "./session.ts";
import { promptParts } from "./promptparts-state.ts";
import { parentRow } from "./nesting-state.ts";
import { sessions } from "./sessions.ts";
import { fillAgentPics } from "./images.ts";
import { picTile } from "./pic-state.ts";
import { filesHtml, planHtml, questionHtml } from "./coderhtml.ts";
import { drawModelPill } from "./modelpill.ts";
import { drawModes } from "./modepill.ts";
import { backgroundedEntry, drawTasks } from "./tasks.ts";
import { drawQueue } from "./messages.ts";
import { olderNote } from "./older-state.ts";
import type { ChatBody } from "../ui/turn.ts";
import { ICON } from "../ui/icons.ts";

/// A step's mark: done, failed, running or did not finish.
const mark = (status: string) => stepMark(status, AGENT_ACTIVE.has(AG.page?.session.status));

/// A tool entry as one line: its mark and a short label ("Ran cargo test").
export function toolRow(en: AgentEntry, at: number, pics = "") {
  const log = AG.log!;
  const { title, status, output } = log.parseTool(en.text);
  if (log.subagent(output)) return subCard([at]) + (pics ? `<div class="dcoderen tool">${pics}</div>` : "");
  const failed = status === "failed", open = stepOpen(failed, at === newestStep, AG.steps.get(at));
  // A command in the background runs on after its turn ended.
  const live = status === "in_progress" && (AGENT_ACTIVE.has(AG.page?.session.status) || backgroundedEntry(at));
  const label = stepLabel(en.text, live, open);
  const exit = failed ? log.exitCode(output) : null;
  const beep = beepCall(title);
  const long = title.includes("\n") || label.length > 80;
  const tail = log.tailLines(output), whole = AG.all.has(at);
  // A Beep call's row shows the first line of its command or prompt; opened, all of it.
  const body = !open ? ""
    : (beep?.detail ? `<pre class="cmd">${esc(beep.detail)}</pre>` : "")
      + (output ? `<pre class="out${whole ? " all" : ""}">${esc(whole ? output : tail.shown)}</pre>` : "")
      + (tail.cut ? `<button class="more" data-all="${at}">${whole ? "Show less" : `Show all ${tail.total === 1 ? "of it" : `${tail.total} lines`}`}</button>` : "");
  const parts = rowParts(en.text, live, open);
  // A command may be cut at any width, so it always opens.
  const opens = !!output || long || (beep ? !!beep.detail : parts.kind === "command");
  const published = AG.page ? toolArtifacts(en.text, AG.page.session.cwd).files.find(f => f.published) : undefined;
  const artifact = published ? ` data-artifact="${esc(published.path)}" title="Open in the Artifacts tab"` : "";
  const state = failed ? (exit != null ? `exit ${exit}` : "Failed") : status === "completed" ? "Done" : live ? "Running" : "";
  if (beep) {
    return `<div class="dcoderen tool beep${failed ? " failed" : ""}${open ? " open" : ""}${published ? " artifact" : ""}" data-row="${at}"${opens ? ` data-at="${at}"` : ""}${artifact}>`
      + `<span class="mk">${stepMark(status, live)}</span>${beepHead(beep, title, state, live, open)}`
      + mediaButtons(title) + body + pics + "</div>";
  }
  // Opened, a command or a long title leaves the row's one line and shows whole under it.
  const wrapped = open && (parts.kind === "command" || long);
  const rowBody = !open ? "" : (wrapped ? `<pre class="cmd">${parts.kind === "command" ? cachedCommandHtml(parts.target) : esc(parts.target)}</pre>` : "") + toolBody(output);
  return toolRowHtml({
    ...parts, target: wrapped ? "" : parts.target, tip: commandOf(title), mark: stepMark(status, live),
    cls: `${failed ? " failed" : ""}${open ? " open" : ""}${published ? " artifact" : ""}`,
    attrs: ` data-row="${at}"${opens ? ` data-at="${at}"` : ""}${artifact}`,
    after: mediaButtons(title) + rowBody + pics,
  });
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

/// A call to the session's Beep tools: what it did, the machine as a chip,
/// the first line of its command or prompt, then the file count and how it
/// stands on the right. Opened, the command or prompt is under it instead.
function beepHead(beep: BeepCall, title: string, state: string, live: boolean, open: boolean) {
  return `<span class="tt bp" title="${esc(beepLine(title) ?? title)}"><span class="bi">${KIND_ICONS.beep}</span><span class="v">${esc(live ? beep.doing : beep.done)}</span>`
    + (beep.machine ? `<span class="mc">${esc(beep.machine)}</span>` : "")
    + (beep.detail && !open ? `<code>${esc(beep.detail.split("\n")[0])}</code>` : "") + "</span>"
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
  // An agent another agent started has a session of its own, whose state the node wrote.
  const working = sub.child ? sub.child.state === "working" : (status === "in_progress" || status === "pending") && AGENT_ACTIVE.has(p.session.status);
  // Timed from when this window saw it working, if it did.
  const key = `${AG.open?.session}\n${at}`, now = Date.now();
  let seen = subTimes.get(key);
  if (!seen) subTimes.set(key, seen = { start: now, live: working });
  if (!working && seen.live && seen.end == null) seen.end = now;
  const ms = !seen.live ? null : working ? now - seen.start : (seen.end ?? now) - seen.start;
  return { title: title.split("\n")[0], agent: sub.agent ?? "", status, working, failed: status === "failed", sub, ms };
}

/// One subagent as a row: its name, how it stands, how long and how many steps.
/// In the pane a finished one can be closed.
function subRow(at: number, inPane = false) {
  const x = subOf(at);
  const kid = x.sub.child;
  const st = kid ? kidState(kid.state) : x.working ? "working" : x.failed ? "failed" : "done";
  const spent = kid?.tokens ? [`${tokenCount(kid.tokens)} tokens`, kid.cost != null ? money(kid.cost) : ""] : [];
  const meta = [st, x.ms != null ? duration(x.ms) : "", kid ? "" : x.sub.count === 1 ? "1 step" : `${x.sub.count} steps`, ...spent].filter(Boolean).join(" · ");
  const kind = x.agent ? `<span class="kind">${esc(x.agent)}</span>` : "";
  // A child opens as the session it is; a subagent opens in this one.
  const opens = kid ? `data-kid="${esc(kid.id)}" data-key="${esc(kid.key)}"` : `data-sub="${at}"`;
  // A child that is done still holds its session on the machine; closing the row closes it.
  const close = inPane && st !== "working" && st !== "needs you" ? `<em class="x" data-subclose="${at}"${st === "done" && kid ? " data-endkid" : ""} title="Close">${ICON.x}</em>` : "";
  return `<button class="dsubrow" ${opens} title="Open ${esc(x.title)}"><span class="mk">${mark(x.working ? "in_progress" : x.status)}</span><span class="t"><b>${esc(x.title)}${kind}</b><small>${esc(meta)}</small></span>${close}</button>`;
}

/// Subagents closed in the pane, by session and the call's place. The log keeps them.
const CLOSED_SUBS_KEY = "closed-subagents";
const CLOSED_SUBS_KEPT = 500;
const closedSubs = new Set<string>();
try { for (const key of JSON.parse(localStorage.getItem(CLOSED_SUBS_KEY) ?? "[]")) closedSubs.add(key); } catch { /* none kept */ }

/// The pane's list: the subagents shown in the log, less those closed.
function paneSubs(p: AgentPage) {
  const open = subagentsShown(p).filter(at => !closedSubs.has(`${AG.open?.session}
${at}`));
  return { html: open.map(at => subRow(at, true)).join(""), count: open.length };
}

/// Takes a finished subagent's row out of the pane.
export function closeSub(at: number) {
  closedSubs.add(`${AG.open?.session}
${at}`);
  try { localStorage.setItem(CLOSED_SUBS_KEY, JSON.stringify([...closedSubs].slice(-CLOSED_SUBS_KEPT))); } catch { /* kept for this launch only */ }
  if (AG.page) drawPane(paneSubs(AG.page));
}

/// How an agent another agent started stands, as its card says.
function kidState(state: string) {
  return state === "working" ? "working" : state === "waiting_on_user" ? "needs you" : state === "ended" ? "ended" : "done";
}

/// Subagents started together as one card: "3 subagents · 2 working · 1 done".
function subCard(ats: number[]) {
  const all = ats.map(subOf), working = all.filter(x => x.working).length, failed = all.filter(x => x.failed).length;
  const done = all.length - working - failed;
  const counts = [working && `${working} working`, done && `${done} done`, failed && `${failed} failed`].filter(Boolean).join(" · ");
  const head = ats.length > 1 ? `<div class="h">${SUB_ICON}<b>${ats.length} ${all.every(x => x.sub.child) ? "agents started" : "subagents"}</b><span>${esc(counts)}</span></div>` : "";
  return `<div class="dsubgrp" data-row="${ats[0]}">${head}${ats.map(at => subRow(at)).join("")}</div>`;
}

/// The places of the subagents' calls among the entries shown.
function subagentsShown(p: AgentPage) {
  if (!AG.log) return [];
  return p.entries.flatMap((en, i) => en.kind === "tool" && AG.log!.subagent(AG.log!.parseTool(en.text).output) ? [p.earlier + i] : []);
}

/// A subagent opened as its own conversation: the way back, what it was
/// asked, its steps and what it answered.
/// What a subagent's log holds.
function childHtml(p: AgentPage, at: number) {
  const x = subOf(at), s = p.session, k = agentKind(s.agent);
  const steps = x.sub.steps.map(st => `<div class="dcoderen tool"><span class="mk">${mark(st.status)}</span><code class="tt" title="${esc(st.title)}">${esc(stepLabel(`${st.title} [${st.status}]`, st.status === "in_progress" && x.working))}</code></div>`).join("");
  const earlier = x.sub.count > x.sub.steps.length ? `<div class="dmuted">${x.sub.count - x.sub.steps.length} earlier steps not shown</div>` : "";
  const none = !x.sub.count && !x.working ? `<div class="dcoderen other">Steps not reported by ${esc(k.name)}.</div>` : "";
  const result = x.sub.result ? `<div class="dcoderen agent">${esc(x.sub.result)}</div>` : x.working ? '<div class="dmuted"><span class="dspin"></span> Working…</div>' : "";
  return `<div class="dcrumb"><button id="agparent">${esc(s.title || "Session")}</button><span>›</span><b>${esc(x.title)}</b><span class="sp"></span><button class="dbtn" id="agparent2" data-parent="1">Open parent<kbd>esc</kbd></button></div>
    <div class="dcoderen user"><b>${esc(x.agent || "Task")}</b><span>${esc(x.title)}</span></div>
    ${turnHeadHtml({ state: x.working ? "working" : "done", ms: x.ms, stats: "", key: null, opened: false })}
    ${earlier}${steps}${none}${result}`;
}

/// The line back to the session whose agent started this one, when one did:
/// its parent and this session's title, and the way up.
function upCrumb(p: AgentPage) {
  const up = parentRow(sessions, { machine_id: p.session.machine_id, id: p.session.id });
  if (!up) return "";
  return `<div class="dcrumb"><button id="agup">${esc(up.title || "Session")}</button><span>›</span><b>${esc(p.session.title || "Session")}</b><span class="sp"></span><button class="dbtn" id="agup2">Open parent<kbd>esc</kbd></button></div>`;
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
  const key = /^image-([0-9a-f]{16})$/.exec(n)?.[1];
  const sent = (AG.open && agentPics.get(`${AG.open.session}\n${n}`)) || undefined;
  return picTile(n, sent, key ? agentThumbs.get(key) : undefined);
}

/// The thought still arriving, by its place, which shows "Thinking"; set
/// by each draw of the turns.
let liveAt: number | null = null;
/// The thought that streamed at the draw before, which folds as it ends.
let wasLive: number | null = null;
/// The running turn thinks: a thought streams or is about to.
export let thinking = false;

/// A thought as one line that opens to its text. While it streams it
/// follows its newest line; once done it folds to "Thought for 12s" unless
/// the reader opened, folded or scrolled it.
function thoughtRow(en: AgentEntry, at: number) {
  const live = at === liveAt, session = AG.open?.session ?? "";
  const m = AG.open ? turnsOf(AG.open.session) : null;
  const touched = !!m?.touched.has(at), opened = !!m?.thoughts.has(at);
  const folding = !live && !touched && wasLive === at;
  const open = live ? !touched || opened : opened || folding;
  return thoughtHtml({ key: `${session}:${at}`, text: en.text, html: en.html, ms: en.ms, live, open, folding, row: at });
}

export function entryRow(en: AgentEntry, at: number) {
  const text = esc(en.text);
  if (en.kind === "thought") return thoughtRow(en, at);
  // Codex echoes answered questions as a message: show them as answered.
  const answered = en.kind === "user" && AG.log ? AG.log.answeredQuestions(en.text) : null;
  if (answered) return `<div class="dcoderen qa" data-row="${at}">${answered.map(x => `${x.question ? `<span class="q">${esc(x.question)}</span>` : ""}<span class="a">${esc(x.answer)}</span>`).join("")}</div>`;
  if (en.kind === "user") {
    // Beep's context and what another session sent get rows of their own,
    // never the user's box.
    const parts = promptParts(en.text);
    const others = parts.filter(p => p.kind !== "user").map(p => p.kind === "context"
      ? `<details class="dcoderen beepctx"><summary>Beep context</summary><pre>${esc(p.text)}</pre></details>`
      : `<div class="dcoderen fromsession"><b>Other session wrote${p.name ? `<span class="who">${esc(p.name)}</span>` : ""}</b><span>${linkified(p.text)}</span></div>`).join("");
    const own = parts.filter(p => p.kind === "user").map(p => p.text).join("\n");
    if (!own && others) return others;
    const { text: said, names } = userImages(own);
    const user = (s: string) => AG.log ? AG.log.userHtml(s, linkified, i => AG.opened.has(`${at}:${i}`)) : linkified(s);
    return others + userRowHtml(user(said), ` data-at="${at}"`, names.length ? `<span class="pics">${names.map(entryPic).join("")}</span>` : "");
  }
  // The pictures the agent or a tool showed go after the text, as a
  // user's do. The HTML is already without their lines.
  const { text: said, names } = AG.log ? AG.log.imageLines(en.text) : { text: en.text, names: [] };
  const pics = names.length ? `<span class="pics">${names.map(entryPic).join("")}</span>` : "";
  // The HTML is the desktop's own Markdown render, cleaned of anything that runs.
  // Image files the reply names link to its pictures, shown in a strip.
  const linked = (html: string) => AG.log ? AG.log.markWarnings(AG.log.linkImagePaths(html).html) : html;
  if (en.kind === "agent") return en.html ? `<div class="dcoderen agent md" data-row="${at}">${answerHtml(linked(en.html))}${pics}</div>` : `<div class="dcoderen agent" data-row="${at}">${linked(esc(said))}${pics}</div>`;
  if (en.kind === "tool") return AG.log ? toolRow({ ...en, text: said }, at, pics) : `<div class="dcoderen tool">${text}</div>`;
  return `<div class="dcoderen other" data-row="${at}">${esc(madeNote(en.text) ?? en.text)}</div>`;
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
  // A turn older than the ones known was read from further back, not seen starting.
  const newest = Math.max(-1, ...m.known);
  for (const t of turns) {
    if (!m.known.has(t.key) && !first && t.key > newest) m.starts.set(t.key, now);
    m.known.add(t.key);
  }
  if (m.running != null && m.running !== running && !m.ends.has(m.running)) m.ends.set(m.running, now);
  m.running = running;
}

/// The places of the tool rows the reader opened.
const openedSteps = () => new Set([...AG.steps].filter(([, open]) => open).map(([at]) => at));

/// The newest step of the running turn, which starts open when it failed.
export let newestStep: number | null = null;

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
  for (const group of AG.log ? toolRuns(p.entries, ats, p.earlier, openedSteps()) : ats.map(at => [at])) {
    if (group.length > 1) { flush(); out.push(runRow(p, group)); continue; }
    const at = group[0];
    if (isSub(at)) { run.push(at); continue; }
    flush();
    out.push(entryRow(p.entries[at - p.earlier], at));
  }
  flush();
  return out;
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
/// worked, then its work and its answer. A turn's rows are worked out only
/// when it is drawn (logwindow.ts).
function turnUnits(p: AgentPage): Unit[] {
  const s = p.session, active = AGENT_ACTIVE.has(s.status);
  newestStep = null;
  thinking = false;
  turnStarts.clear();
  const turns = splitTurns(p.entries, p.earlier), lastTurn = turns.at(-1);
  const m = turnsOf(AG.open?.session ?? s.id);
  noteTurns(m, turns, active && lastTurn ? lastTurn.key : null);
  const row = (at: number) => entryRow(p.entries[at - p.earlier], at);
  const entry = (at: number) => p.entries[at - p.earlier];
  wasLive = m.wasLive;
  liveAt = lastTurn ? liveThought(p.entries, lastTurn, active, p.earlier) : null;
  m.wasLive = liveAt;
  return turns.map(t => ({ key: `t${t.key}`, rows: turnRows(t) }));

  function turnRows(t: Turn): () => string[] {
    // Notes before the first message are not a turn of work.
    if (onlyNotes(p.entries, t, p.earlier)) return () => t.steps.map(row);
    const last = t === lastTurn, running = active && last;
    const user = () => t.user == null ? "" : row(t.user);
    if (!running && !t.steps.length && !t.answer.length) return () => [user()].filter(Boolean);
    const { start, end } = span({
      first: p.earlier === 0 && t.key === 0, last, running,
      seenStart: m.starts.get(t.key), seenEnd: m.ends.get(t.key),
      sessionStart: s.started_at, lastActivity: s.last_activity,
    });
    if (start != null) turnStarts.set(t.key, start);
    const state: TurnState = !running ? "done" : s.status === "asking" ? "waiting" : "working";
    const ms = start == null ? null : running ? Date.now() - start : end == null ? null : end - start;
    // The newest step of a running turn, shown as it goes.
    const newest = running ? [...t.steps].reverse().find(at => entry(at).kind === "tool") : undefined;
    if (newest != null) newestStep = newest;
    // What the agent thinks or does now, on one dim line, unless the
    // newest step says it already.
    const doing = s.doing?.split("\n")[0].trim() ?? "";
    if (running) thinking = liveAt != null || doing === "Thinking";
    return () => {
      const opened = m.opened.has(t.key);
      let work: string[], stats = turnStats(p.entries, t, p.earlier);
      if (opened) work = rowsOf(p, t.steps);
      else {
        const kept = new Set(keptSteps(p.entries, t, p.earlier, openedSteps()));
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
        work = [pics, ...rowsOf(p, t.steps.filter(at => kept.has(at)))];
      }
      const head = turnHeadHtml({ state, start, ms, stats, key: t.steps.length ? t.key : null, opened });
      const shownTitle = newest != null ? AG.log?.parseTool(entry(newest).text).title.trim() : "";
      const live = running ? liveLineHtml({
        state, thinking: thinking, start,
        doing: state === "working" && doing !== "Thinking" && doing !== shownTitle && liveAt == null ? doing : "",
      }) : "";
      return [user(), head, ...work, ...t.answer.map(row), live].filter(Boolean);
    };
  }
}

/// The question's answers; the first that allows and the first that does
/// not take ⌘Return and ⌘D.
function questionCard(q: AgentQuestion) {
  return questionHtml(q, "data-agopt", q.id === AG.answering, true);
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

/// The row in view at the top of the log, to pass to `drawAgentBody` as the
/// row that stays where it is when a key opens or folds many rows at once.
export function rowInView() {
  const b = $<ChatBody>("#agbody");
  return (b && scrollAnchor(b).sel) || "[data-row]";
}

/// Draws the log again. `keep` finds the row the reader just opened or
/// folded, which stays where it is on screen.
export function drawAgentBody(keep?: string, polled = false) {
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
  // A plan waiting for approval shows above the field instead.
  const question = q && AG.modes?.plan == null ? questionCard(q) : "";
  const plan = planHtml(p.plan, s.status === "asking");
  const files = filesHtml(p.files, 12);
  // Steers sent from here show above the field; this counts the others.
  const others = p.queued - AG.queue.filter(q => q.state === "sent" || q.state === "sending").length;
  const queued = others > 0 ? `<div class="row note">${others === 1 ? "1 message waits" : `${others} messages wait`} for the turn to end</div>` : "";
  // The question, plan and files sit under the log, by the field.
  const card = question || plan || files || queued
    ? `<div class="dcard dcoder">${question}${plan}${files}${queued}</div>` : "";
  const why = AG.why ? `<div class="dmuted">Auto picked ${esc(s.machine_name)}. ${esc(AG.why)}</div>` : "";
  const olderText = olderNote(p, AG.older.stop, AG.older.loading);
  const earlier = olderText ? `<div class="dmuted">${esc(olderText)}</div>` : "";
  // The log goes in by turns, and only those near the view are drawn.
  const units = child != null ? [fixed("child", childHtml(p, child))]
    : p.entries.length && AG.log ? [fixed("up", upCrumb(p)), fixed("older", earlier), ...turnUnits(p)] : null;
  const entries = units ? '<div class="dcoderlog"></div>'
    : s.status === "starting" ? '<div class="dmuted"><span class="dspin"></span> Starting the agent…</div>' : "";
  // What stopped an outside session's log from reading is not a failed turn.
  const err = p.error ? (s.external ? `<div class="derr">${esc(p.error)}</div>` : errorCardHtml(p.error)) : "";
  const note = AG.note === "gone" ? '<div class="dmuted">This session is no longer on the machine, or the machine went offline.</div>'
    : AG.note && AG.note === AG.readError && AG.note.startsWith("Reconnecting") ? `<div class="dmuted">${esc(AG.note)}</div>`
    : AG.note ? `<div class="derr">${esc(AG.note)}</div>` : "";
  const paused = AG.paused ? '<button class="dbtn quiet" id="agresume">Paused, click to refresh</button>' : "";
  const subs = child == null ? paneSubs(p) : { html: "", count: 0 };
  if (!showLog(b, `${why}${entries}${child == null ? card : ""}${err}${note}${paused}`, units, polled)) { AG.rest = ""; return; }
  // The pane follows the agent, or the subagent opened, into a worktree.
  followAgent(p.entries, p.earlier, child != null ? subOf(child).sub.steps.map(st => st.title) : null);
  const arts = child == null ? { list: collectArtifacts(p.entries, p.earlier, s.cwd), turnStart: (k: number) => turnStarts.get(k) } : undefined;
  drawPane(subs, arts);
  drawModelPill();
  drawModes();
  drawTasks();
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
      stop.innerHTML = stopFace(stopping);
    }
  }
  if (scroll === "bottom") b.scrollTop = b.scrollHeight;
  else if (anchor) restoreScroll(b, anchor);
  // A row opened at the bottom pushes the bottom away: the log follows
  // new entries again once the reader scrolls back down.
  if (keep != null) b.stuck = atBottom(b);
  fillLog(b, scroll === "bottom");
  drawnRows(b);
  syncJump(b);
}

/// A part of the log that is not a turn, as one; an empty one is hidden.
const fixed = (key: string, html: string): Unit => ({ key, rows: () => html ? [html] : [] });

/// What rows just drawn need: their pictures and their live thoughts.
function drawnRows(b: HTMLElement) {
  fillAgentPics(b);
  attachLive(b, key => { if (AG.open) turnsOf(AG.open.session).touched.add(Number(key.split(":").pop())); });
}

/// The log scrolled: the turns it reaches are drawn. At the bottom it stays there.
export function scrolledLog(b: ChatBody) {
  if (fillLog(b, b.stuck !== false)) drawnRows(b);
}

/// Puts the log's html in the page, replacing only the rows that changed.
/// A poll leaves a row the reader is selecting in place until it is
/// unselected: false then, with that row as it was.
function showLog(b: HTMLElement, html: string, units: Unit[] | null, polled: boolean) {
  let main = b.querySelector<HTMLElement>(":scope > .dagmain");
  if (!main || !patchable(main)) {
    b.innerHTML = `<div class="dagmain"></div><button class="dagjump" id="agjump" hidden>Jump to latest${DOWN_ICON}</button>`;
    main = b.firstElementChild as HTMLElement;
    startPatching(main);
  }
  const next = document.createElement("template");
  next.innerHTML = html;
  const sel = getSelection(), held: Node[] = [];
  if (polled && sel && (!sel.isCollapsed || b.matches(":active"))) {
    if (sel.anchorNode) held.push(sel.anchorNode);
    if (sel.focusNode) held.push(sel.focusNode);
  }
  return patchRows(main, next.content, units, held);
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
  // From far up it goes at once: a glide would end short of turns measured on the way.
  const far = b.scrollHeight - b.scrollTop - b.clientHeight > b.clientHeight;
  b.scrollTo({ top: b.scrollHeight, behavior: far || matchMedia("(prefers-reduced-motion: reduce)").matches ? "auto" : "smooth" });
  syncJump(b);
}
