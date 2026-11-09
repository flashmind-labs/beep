// A coding agent session's log as the bar draws it: turns, tool steps,
// thoughts, subagents and questions, and where it scrolls.
import { toolDetailsHtml } from "./tool-details.ts";
import { isCommandCall } from "./tool-details-state.ts";
import { esc, linkified, money, tokenCount } from "../ui/format.ts";
import { type BeepCall, beepCall, beepLine, commandOf, diffBlocks, duration, keptSteps, liveThought, onlyNotes, rowParts, runLabel, span, splitTurns, stepLabel, stepOpen, progressSummary, toolKind, toolRuns, turnSections, type Turn, type TurnState, withoutDiffs } from "./agentturns.ts";
import { cachedCommandHtml } from "./command-state.ts";
import { $, S } from "../bar/core.ts";
import { drawPane } from "../agentpane/media.ts";
import { followAgent } from "../agentpane/worktree.ts";
import { collectArtifacts, type MadeFile, type MadeFiles, madeFiles, toolArtifacts } from "../agentpane/artifacts-state.ts";
import { atBottom, keptScrollTop, logScroll } from "./logscroll.ts";
import { attachLive } from "./thoughtlive.ts";
import { groupThoughts, mergedThought } from "./thoughts-state.ts";
import { patchable, patchRows, startPatching } from "./logpatch.ts";
import { fillLog, pinEnd, type Unit, type TranscriptRow } from "./logwindow.ts";
import { answerHtml, collapseActivityHtml, errorCardHtml, KIND_ICONS, liveLineHtml, stepMark, stopFace, thoughtHtml, tickTurns, toolRowHtml, turnHeadHtml, userRowHtml } from "./turnrows.ts";
import { type AgentQuestion, type AgentEntry, type AgentPage, AG, agentLog, agentPics, agentThumbs, AGENT_ACTIVE, agentAvatar, agentKind, stateTag } from "./agents.ts";
import { stoppingNow, redrawAgentHints, userImages } from "./session.ts";
import { promptParts } from "./promptparts-state.ts";
import { childRow } from "./nesting-state.ts";
import { childReplyHtml } from "./childreply-state.ts";
import { parentSession, sessions } from "./sessions.ts";
import { fillAgentPics } from "./images.ts";
import { prepareInlineImages } from "./inline-images.ts";
import { picTile } from "./pic-state.ts";
import { planHtml, questionHtml } from "./coderhtml.ts";
import { drawModelPill } from "./modelpill.ts";
import { drawModes } from "./modepill.ts";
import { backgroundedEntry, drawTasks } from "./tasks.ts";
import { drawQueue } from "./messages.ts";
import { olderNote } from "./older-state.ts";
import { holdNote } from "./reconnect-state.ts";
import type { ChatBody } from "../ui/turn.ts";
import { ICON } from "../ui/icons.ts";
import { replyHtml, type ReplyRow } from "./reply-view.tsx";

// Elapsed clocks use when this window first saw the call running.
const toolStarts = new Map<string, number>();

/// A tool entry as one line: its mark and a short label ("Ran cargo test").
export function toolRow(en: AgentEntry, at: number, pics = "") {
  const log = AG.log!;
  const { title, status, output } = log.parseTool(log.imageLines(en.text).text);
  if (log.subagent(output)) return subCard([at]) + (pics ? `<div class="dcoderen tool">${pics}</div>` : "");
  const failed = status === "failed", open = stepOpen(failed, at === newestStep, AG.steps.get(at));
  // A command in the background runs on after its turn ended.
  const live = status === "in_progress" && (AGENT_ACTIVE.has(AG.page?.session.status) || backgroundedEntry(at));
  const label = stepLabel(en.text, live, open);
  const exit = failed ? log.exitCode(output) : null;
  const beep = beepCall(title);
  const long = title.includes("\n") || label.length > 80;
  const parts = rowParts(en.text, live, open);
  const inspected = !beep && isCommandCall(en.tool) && (!!en.tool?.name || parts.kind === "command");
  const tail = log.tailLines(output), whole = AG.all.has(at);
  // A Beep call's row shows the first line of its command or prompt; opened, all of it.
  const body = !open ? ""
    : (beep?.detail ? `<pre class="cmd">${esc(beep.detail)}</pre>` : "")
      + (output ? `<pre class="out${whole ? " all" : ""}">${esc(whole ? output : tail.shown)}</pre>` : "")
      + (tail.cut ? `<button class="more" data-all="${at}">${whole ? "Show less" : `Show all ${tail.total === 1 ? "of it" : `${tail.total} lines`}`}</button>` : "");
  const key = `${AG.open?.machine_id ?? ""}\n${AG.open?.session ?? ""}\n${at}`;
  if (live && !toolStarts.has(key)) toolStarts.set(key, Date.now());
  const since = live ? toolStarts.get(key) : undefined;
  if (!live) toolStarts.delete(key);
  // A command may be cut at any width, so it always opens.
  const opens = inspected || !!output || long || (beep ? !!beep.detail : parts.kind === "command");
  const toggleAttrs = opens ? ` data-at="${at}"${inspected || parts.kind === "command" ? ` tabindex="0" role="button" aria-expanded="${open}"` : ""}` : "";
  const inspectedBody = open && inspected ? toolDetailsHtml(en.tool, status, live, output, diffBlocks(output).length ? toolBody(output) : undefined) : "";
  const published = AG.page ? toolArtifacts(en.text, AG.page.session.cwd).files.find(f => f.published) : undefined;
  const artifact = published ? ` data-artifact="${esc(published.path)}" title="Open in the Artifacts tab"` : "";
  const state = failed ? (exit != null ? `exit ${exit}` : "Failed") : status === "completed" ? "Done" : live ? "Running" : "";

  if (beep) {
    return `<div class="dcoderen tool beep${failed ? " failed" : ""}${open ? " open" : ""}${published ? " artifact" : ""}" data-row="${at}"${toggleAttrs}${artifact}>`
      + `<span class="mk">${stepMark(status, live)}</span>${beepHead(beep, title, state, live, open, since)}`
      + mediaButtons(title) + body + pics + "</div>";
  }

  // Opened, a command or a long title leaves the row's one line and shows whole under it.
  const wrapped = open && (parts.kind === "command" || long);
  const rowBody = !open ? "" : (wrapped ? `<pre class="cmd">${parts.kind === "command" ? cachedCommandHtml(parts.target) : esc(parts.target)}</pre>` : "") + toolBody(output);
  return toolRowHtml({
    ...parts,
    since, target: inspected ? parts.target : wrapped ? "" : parts.target, tip: commandOf(title), mark: stepMark(status, live),
    cls: `${failed ? " failed" : ""}${open ? " open" : ""}${published ? " artifact" : ""}`,
    attrs: ` data-row="${at}"${toggleAttrs}${artifact}`,
    after: mediaButtons(title) + (inspected ? inspectedBody : rowBody) + pics,
  });
}

function commandClock(since: number) {
  return `<span data-since="${since}" title="Time since this window saw the tool running">${duration(Date.now() - since)}</span>`;
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
function beepHead(beep: BeepCall, title: string, state: string, live: boolean, open: boolean, since?: number) {
  return `<span class="tt bp" title="${esc(beepLine(title) ?? title)}"><span class="bi">${KIND_ICONS.beep}</span><span class="v">${esc(live ? beep.doing : beep.done)}</span>`
    + (beep.machine ? `<span class="mc">${esc(beep.machine)}</span>` : "")
    + (beep.detail && !open ? `<code>${esc(beep.detail.split("\n")[0])}</code>` : "") + "</span>"
    + `<span class="bs">${esc([beep.files, state].filter(Boolean).join(" · "))}${since != null ? ` · ${commandClock(since)}` : ""}</span>`;
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
  // A linked subagent can keep working after its parent's turn ends.
  const working = sub.child ? sub.child.state === "working" : (status === "in_progress" || status === "pending") && (!!sub.session || AGENT_ACTIVE.has(p.session.status));
  // Timed from when this window saw it working, if it did.
  const key = `${AG.open?.session}\n${at}`, now = Date.now();
  let seen = subTimes.get(key);
  if (!seen) subTimes.set(key, seen = { start: now, live: working });
  if (!working && seen.live && seen.end == null) seen.end = now;
  const ms = !seen.live ? null : working ? now - seen.start : (seen.end ?? now) - seen.start;
  const [name, ...prompt] = title.split("\n");
  return { title: name, prompt: sub.child ? prompt.join("\n") : "", agent: sub.agent ?? "", status, working, failed: status === "failed", sub, ms };
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
  const opens = kid ? `data-kid="${esc(kid.id)}" data-key="${esc(kid.key)}"` : x.sub.session ? `data-native="${at}"` : `data-subsummary="${at}"`;
  const close = inPane && st !== "working" && st !== "needs you" ? `<em class="x" data-subclose="${at}" title="Dismiss from pane">${ICON.x}</em>` : "";
  // A child that runs can be stopped from its row, as from its own log.
  const stoppable = kid && (st === "working" || st === "needs you");
  const stop = stoppable ? `<em class="kstop" data-kidstop title="Stop ${esc(x.title)}">Stop</em>` : "";
  const row = `<button class="dsubrow${stoppable ? " stoppable" : ""}" ${opens} title="Open ${esc(x.title)}"><span class="mk">${stepMark(x.working ? "in_progress" : x.status, x.working)}</span><span class="t"><b>${esc(x.title)}${kind}</b><small>${esc(meta)}</small></span>${close}${stop}</button>`;
  if (!kid && !x.sub.session && AG.steps.get(at)) return row + `<div class="dmuted">${esc(x.sub.result || "The agent has not provided a readable conversation yet.")}</div>`;
  if (inPane || !x.prompt) return row;
  const open = AG.steps.get(at) ?? false;
  return `<div class="dsubprompt" data-row="${at}" data-at="${at}">${row}<button class="prompt-toggle" data-prompt="${at}" aria-expanded="${open}">${ICON.chev}Prompt</button>${open ? `<pre>${esc(x.prompt)}</pre>` : ""}</div>`;
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

/// The line back to the session whose agent started this one, when one did:
/// its parent and this session's title, and the way up.
function upCrumb(p: AgentPage) {
  const up = parentSession(p.session);
  if (!up) return "";
  return `<div class="dcrumb"><button id="agup">${esc(up.title || "Session")}</button><span>/</span><b>${esc(p.session.title || "Session")}</b><span class="sp"></span><button class="dbtn" id="agup2">Open parent<kbd>esc</kbd></button></div>`;
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
/// The running turn thinks: a thought streams or is about to.
export let thinking = false;

/// Thoughts start folded because the progress line already shows what
/// the agent thinks. The reader's choice stays as the group grows or ends.
function thoughtRow(en: AgentEntry, at: number, live = at === liveAt) {
  const session = AG.open?.session ?? "";
  const m = AG.open ? turnsOf(AG.open.session) : null;
  return thoughtHtml({ key: `${session}:${at}`, text: en.text, html: en.html, ms: en.ms, live, open: !!m?.thoughts.has(at), folding: false, row: at });
}

export function entryRow(en: AgentEntry, at: number) {
  const text = esc(en.text);
  if (en.kind === "thought") return thoughtRow(en, at);
  // Codex echoes answered questions as a message: show them as answered.
  const answered = en.kind === "user" && AG.log ? AG.log.answeredQuestions(en.text) : null;
  if (answered) return `<div class="dcoderen qa" data-row="${at}">${answered.map(x => `${x.question ? `<span class="q">${esc(x.question)}</span>` : ""}<span class="a">${esc(x.answer)}</span>`).join("")}</div>`;

  if (en.kind === "user") {
    // Beep's context, what another session sent and what a child came to
    // get rows of their own, never the user's box.
    const parts = promptParts(en.text);
    let child = 0;
    const others = parts.map(p => p.kind === "context"
      ? `<details class="dcoderen beepctx"><summary>Beep context</summary><pre>${esc(p.text)}</pre></details>`
      : p.kind === "session"
        ? `<div class="dcoderen fromsession"><b>Other session wrote${p.name ? `<span class="who">${esc(p.name)}</span>` : ""}</b><span>${linkified(p.text)}</span></div>`
        : p.kind === "child"
          ? (() => {
            const row = childRow(sessions, p.id);
            const meta = [row?.machine_name ?? p.id.split("@").slice(1).join("@"), row ? agentKind(row.agent).name : ""].filter(Boolean).join(" · ");
            const body = en.child_html?.[child++];
            return childReplyHtml(p, body != null ? answerHtml(body) : `<p>${linkified(p.text).replace(/\n/g, "<br>")}</p>`, row ? agentAvatar(row.agent) : SUB_ICON, meta);
          })()
          : "").join("");
    const own = parts.filter(p => p.kind === "user").map(p => p.text).join("\n");
    if (!own && others) return others;
    const { names, text: words } = userImages(own);
    // The user's words come as Markdown HTML, its pasted blocks folded;
    // the ones the reader opened stay open.
    // A message just sent has no HTML until the next read: its text shows meanwhile.
    const html = en.html ?? (words ? `<p>${esc(words).replace(/\n/g, "<br>")}</p>` : "");
    const said = html.replace(/data-paste="(\d+)"/g, (m, i) => AG.opened.has(`${at}:${i}`) ? `${m} open` : m);
    return others + userRowHtml(said, ` data-at="${at}"`, names.length ? `<span class="pics">${names.map(entryPic).join("")}</span>` : "", true);
  }

  if (en.kind === "agent") return replyHtml(agentReply(en, at));

  // The pictures the agent or a tool showed go after the text, as a
  // user's do. The HTML is already without their lines.
  const { text: said, names } = AG.log ? AG.log.imageLines(en.text) : { text: en.text, names: [] };
  const pics = names.length ? `<span class="pics">${names.map(entryPic).join("")}</span>` : "";
  if (en.kind === "tool") return AG.log ? toolRow({ ...en, text: said }, at, pics) : `<div class="dcoderen tool">${text}</div>`;
  if (en.kind === "other" && en.text.trim() === "[conversation compacted]") return `<div class="dcoderen compacted" data-row="${at}"><span>Conversation compacted</span></div>`;
  const made = AG.page ? madeFiles(en.text, AG.page.session.cwd) : null;
  if (made) return madeRow(made, at);
  return `<div class="dcoderen other" data-row="${at}">${esc(en.text)}</div>`;
}

/// The files a turn's commands made: one file as "Made x.md", several as a
/// folded "Made 11 files" that opens to their names by kind. A name opens
/// the file in the side pane.
function madeRow(made: MadeFiles, at: number) {
  const chip = (f: MadeFile, cls: string) => `<button class="${cls}" data-made="${esc(f.path)}" title="${esc(f.path)}">${esc(f.name)}</button>`;
  if (made.files.length === 1) return `<div class="dcoderen other mf" data-row="${at}">Made ${chip(made.files[0], "mf-name")}</div>`;
  const open = !!AG.open && turnsOf(AG.open.session).made.has(at);
  const groups = made.groups.map(g => `<div class="mf-grp"><small>${esc(g.label)}</small><div class="mf-chips">${g.files.map(f => chip(f, "mf-chip")).join("")}</div></div>`).join("");
  return `<div class="dcoderen other mf${open ? " open" : ""}" data-row="${at}"><button class="mf-head" data-madeat="${at}" aria-expanded="${open}"><span class="mf-car">›</span>Made ${made.files.length} files<small>${esc(made.counts)}</small></button>${groups}</div>`;
}

/// What this window knows of a session's turns: the ones it has seen, when it
/// saw a turn start or end (entries carry no times), the turn running at the
/// last draw, and the tool sections opened to their steps.
interface TurnMemory {
  known: Set<number>;
  starts: Map<number, number>;
  ends: Map<number, number>;
  running: number | null;
  /// Tool sections opened, by their first entry.
  opened: Set<number>;
  /// The thoughts opened to their text.
  thoughts: Set<number>;
  /// The runs of calls opened, by their first call.
  runs: Set<number>;
  /// The notes of made files opened, by their place in the session.
  made: Set<number>;
}

const turnMemory = new Map<string, TurnMemory>();

export function turnsOf(session: string) {
  let m = turnMemory.get(session);
  if (!m) turnMemory.set(session, m = { known: new Set(), starts: new Map(), ends: new Map(), running: null, opened: new Set(), thoughts: new Set(), runs: new Set(), made: new Set() });
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
/// to them ("Read 6 files"). Adjacent thoughts share one expandable row.
function rowsOf(p: AgentPage, ats: number[]) {
  const out: TranscriptRow[] = [];
  let run: number[] = [];
  const isSub = (at: number) => {
    const en = p.entries[at - p.earlier];
    return en.kind === "tool" && !!AG.log?.subagent(AG.log.parseTool(en.text).output);
  };
  const flush = () => { if (run.length) out.push(subCard(run)); run = []; };
  const groups = AG.log ? toolRuns(p.entries, ats, p.earlier, openedSteps()) : ats.map(at => [at]);

  for (const group of groupThoughts(p.entries, groups, p.earlier)) {
    const at = group[0];

    if (p.entries[at - p.earlier].kind === "thought") {
      flush();
      out.push(thoughtRow(mergedThought(p.entries, group, p.earlier), at, liveAt != null && group.includes(liveAt)));
      continue;
    }

    if (group.length > 1) { flush(); out.push(runRow(p, group)); continue; }
    if (isSub(at)) { run.push(at); continue; }
    flush();
    const en = p.entries[at - p.earlier];
    if (en.kind === "agent") out.push(agentReply(en, at));
    else out.push(entryRow(en, at));
  }

  flush();
  return out;
}

function agentReply(en: AgentEntry, at: number): ReplyRow {
  const { text, names } = AG.log ? AG.log.imageLines(en.text) : { text: en.text, names: [] };
  const pics = names.length ? `<span class="pics">${names.map(entryPic).join("")}</span>` : "";
  const linked = (html: string) => AG.log ? AG.log.markWarnings(AG.log.linkImagePaths(html).html) : html;
  return { kind: "reply", row: String(at), markdown: !!en.html, html: (en.html ? answerHtml(linked(en.html)) : linked(esc(text))) + pics };
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
  liveAt = lastTurn ? liveThought(p.entries, lastTurn, active, p.earlier) : null;
  return turns.map(t => ({ key: `t${t.key}`, rows: turnRows(t) }));

  function turnRows(t: Turn): () => TranscriptRow[] {
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
    // The last tool may need to open on failure inside its progress group.
    const lastStep = t.steps.at(-1);
    const latestTool = t.steps.findLast(at => entry(at).kind === "tool");
    const newest = running && lastStep != null && entry(lastStep).kind === "tool" ? lastStep : undefined;
    if (newest != null) newestStep = newest;
    // What the agent thinks or does now, on one dim line, unless the
    // newest step says it already.
    // Use the last thought only while it is the latest step.
    const thought = running && lastStep != null && entry(lastStep).kind === "thought" ? lastStep : undefined;
    const status = s.doing?.split("\n")[0].trim() ?? "";
    const doing = (thought != null ? thoughtLine(entry(thought).text) : "") || status;
    if (running) thinking = liveAt != null || status === "Thinking";
    return () => {
      const work = turnSections(p.entries, t, p.earlier).flatMap<TranscriptRow>(section => {
        if (!section.progress) return rowsOf(p, section.ats);
        const key = section.ats[0], opened = m.opened.has(key);
        const group: Turn = { key, user: null, steps: section.ats, answer: [] };
        const kept = new Set(keptSteps(p.entries, group, p.earlier, openedSteps()));
        // Keep the latest call in view until the turn ends. A failed last call
        // stays visible afterwards; manually opened rows already stay in kept.
        const preview = latestTool != null && section.ats.includes(latestTool)
          && (running || AG.log?.parseTool(entry(latestTool).text).status === "failed") ? latestTool : null;
        if (preview != null) kept.add(preview);
        const folding = section.ats.filter(at => !kept.has(at));
        const stats = progressSummary(p.entries, group, p.earlier);
        const head = headingRow({ state: "working", ms: null, stats, key, opened });
        const seen = opened ? [] : folding.flatMap(at => AG.log?.imageLines(entry(at).text).names ?? []);
        const pics = seen.length ? `<div class="dcoderen dfoldpics" data-row="${folding[0]}"><span class="pics">${seen.map(entryPic).join("")}</span></div>` : "";
        const rows = rowsOf(p, opened ? section.ats : section.ats.filter(at => kept.has(at)));
        const collapse = opened ? collapseActivityHtml(key) : "";
        return [head, pics, ...rows, collapse];
      });
      const head = headingRow({ state, start, ms, stats: "", key: null, opened: false });
      const shownTitle = newest != null ? AG.log?.parseTool(entry(newest).text).title.trim() : "";
      const live = running ? liveLineHtml({
        state, thinking: thinking, start,
        doing: state === "working" && doing !== "Thinking" && !(shownTitle && (shownTitle.includes(doing) || doing.includes(shownTitle))) ? doing : "",
      }) : "";
      return [user(), head, ...work, live].filter(Boolean);
    };
  }
}

function headingRow(value: Parameters<typeof turnHeadHtml>[0]): TranscriptRow {
  const html = turnHeadHtml(value);
  if (!html) return "";
  return { kind: "heading", id: value.key === null ? "turn" : `steps:${value.key}`, html, state: value.state, start: value.start ?? null, ms: value.ms, stats: value.stats, turn: value.key, opened: value.opened };
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
  const top = el ? keptScrollTop(b.scrollTop, a.off, el.getBoundingClientRect().top - b.getBoundingClientRect().top) : a.scroll;
  if (top !== b.scrollTop) b.scrollTop = top;
}

/// The row in view at the top of the log, to pass to `drawAgentBody` as the
/// row that stays where it is when a key opens or folds many rows at once.
export function rowInView() {
  const b = $<ChatBody>("#agbody");
  return (b && scrollAnchor(b).sel) || "[data-row]";
}

/// Draws the log again. `keep` finds the row the reader just opened or
/// folded, which stays where it is on screen. With `older` it is the row in
/// view while older entries come in above, kept only when the reader is not
/// at the end.
export function drawAgentBody(keep?: string, polled = false, older = false) {
  const b = $<ChatBody>("#agbody"), p = AG.page;
  if (!b || !p) return;
  const s = p.session, q = s.question;
  const st = $("#agstate"), stopping = stoppingNow();
  if (st) st.innerHTML = stopping ? '<span class="dstate run">Stopping</span>' : stateTag(s.status);
  if (!AG.log) agentLog().then(() => { if (S.view === "agent") drawAgentBody(); }).catch(console.warn);
  // Read where the reader is before the log is drawn again: only a reader
  // at the bottom follows new entries; anyone else stays where they are,
  // and so does a row the reader opens or folds.
  const redraw = older ? "older" : keep != null ? "toggle" : "news";
  const scroll = logScroll(b.stuck !== false, redraw);
  const anchor = scroll === "keep" ? scrollAnchor(b, keep) : null;
  // A plan waiting for approval shows above the field instead.
  const question = q && AG.modes?.plan == null ? questionCard(q) : "";
  const plan = planHtml(p.plan, s.status === "asking");
  const card = question || plan
    ? `<div class="dcard dcoder">${question}${plan}</div>` : "";
  const why = AG.why ? `<div class="dmuted">Auto picked ${esc(s.machine_name)}. ${esc(AG.why)}</div>` : "";
  const olderText = olderNote(p, AG.older.stop, AG.older.loading);
  const earlier = olderText ? `<div class="dmuted">${esc(olderText)}</div>` : "";
  // The log goes in by turns, and only those near the view are drawn.
  const units = p.entries.length && AG.log
    ? [fixed("up", upCrumb(p)), fixed("older", earlier), ...turnUnits(p)] : null;
  const entries = units ? '<div class="dcoderlog"></div>'
    : s.status === "starting" ? '<div class="dmuted"><span class="dspin"></span> Starting the agent…</div>' : "";
  // What stopped an outside session's log from reading is not a failed turn.
  const err = p.error ? (s.external ? `<div class="derr">${esc(p.error)}</div>` : errorCardHtml(p.error)) : "";
  const note = AG.held ? `<div class="dmuted">${esc(holdNote(s.machine_name))}</div>`
    : AG.note === "gone" ? '<div class="dmuted">This session is no longer on the machine, or the machine went offline.</div>'
    : AG.note && AG.note === AG.readError && AG.link.lost ? `<div class="dmuted">${esc(AG.note)}</div>`
    : AG.note ? `<div class="derr">${esc(AG.note)}</div>` : "";
  // A read that failed outright is not tried again by itself: Retry reads it.
  const retry = AG.readError && AG.note === AG.readError && !AG.link.lost && AG.note !== "gone";
  const refresh = retry ? '<button class="dbtn quiet" id="agresume">Retry</button>' : "";
  const subs = paneSubs(p);
  if (!showLog(b, `${why}${entries}${card}${err}${note}${refresh}`, units, polled)) { AG.rest = ""; return; }
  // The pane follows the open conversation into its worktree.
  followAgent(p.entries, p.earlier, null);
  const arts = { list: collectArtifacts(p.entries, p.earlier, s.cwd), turnStart: (k: number) => turnStarts.get(k) };
  drawPane(subs, arts);
  drawModelPill();
  drawModes();
  drawTasks();
  drawQueue();
  tickTurns();
  redrawAgentHints();
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

  if (scroll === "bottom") pinEnd(b);
  else if (anchor) restoreScroll(b, anchor);
  // A row opened at the bottom pushes the bottom away: the log follows
  // new entries again once the reader scrolls back down.
  if (redraw === "toggle") b.stuck = atBottom(b);
  fillLog(b, scroll === "bottom");
  drawnRows(b);
  syncJump(b);
}

/// A part of the log that is not a turn, as one; an empty one is hidden.
const fixed = (key: string, html: string): Unit => ({ key, rows: () => html ? [html] : [] });

/// What rows just drawn need: their pictures and their live thoughts.
function drawnRows(b: HTMLElement) {
  fillAgentPics(b);
  attachLive(b);
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
  next.innerHTML = prepareInlineImages(html);
  const sel = getSelection(), held: Node[] = [];
  const focused = document.activeElement;
  if (polled && focused && b.contains(focused)) held.push(focused);

  if (polled && sel && (!sel.isCollapsed || b.matches(":active"))) {
    if (sel.anchorNode) held.push(sel.anchorNode);
    if (sel.focusNode) held.push(sel.focusNode);
  }

  const prepared = units?.map(unit => ({ ...unit, rows: () => unit.rows().map(row => typeof row === "string" ? prepareInlineImages(row) : row.kind === "reply" ? { ...row, html: prepareInlineImages(row.html) } : row) })) ?? null;
  return patchRows(main, next.content, prepared, held);
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

/// The first line of a thought, without its Markdown bold or heading marks.
export function thoughtLine(text: string) {
  const line = text.split("\n").map(l => l.trim()).find(Boolean) ?? "";
  return line.replace(/^#+\s*/, "").replace(/\*\*/g, "").trim();
}
