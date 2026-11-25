import { attachmentPictures, promptAttachments } from "./promptparts-state.ts";
// A coding agent session's log as the bar draws it: turns, tool steps,
// thoughts, subagents and questions, and where it scrolls.
import { answeredQuestions, exitCode, imageLines, linkCodeFiles, linkImagePaths, markWarnings, mediaPaths, parseTool, sentFiles, subagent, tailLines } from "./agentlog.ts";
import { toolDetails } from "./tool-details.ts";
import { isCommandCall } from "./tool-details-state.ts";
import { esc, linkified, money, plural, tokenCount } from "../ui/format.ts";
import { beepCall, beepLine, commandOf, duration, keptSteps, liveThought, onlyNotes, rowParts, runLabel, span, splitTurns, stepLabel, stepOpen, progressSummary, toolKind, toolRuns, turnSections, type Turn, type TurnState } from "./agentturns.ts";
import { cachedCommandHtml } from "./command-state.ts";
import { windowHooks } from "../bar/core.ts";
import { drawPane } from "../agentpane/media-draw.ts";
import type { SubagentRow } from "../agentpane/subagents-view.tsx";
import type { ActivityRow, ToolActivity, ToolBody } from "./activity-view.tsx";
import { followAgent } from "../agentpane/worktree.ts";
import { followDeviceCalls } from "../agentpane/device.ts";
import { collectArtifacts, madeFiles, toolArtifacts } from "../agentpane/artifacts-state.ts";
import { atBottom, keptScrollTop, logScroll } from "./logscroll.ts";
import { attachLive, trackLive } from "./thoughtlive.ts";
import { groupThoughts, mergedThought } from "./thoughts-state.ts";
import { showLogShell } from "./log-shell.tsx";
import type { LogShell } from "./log-shell-view.tsx";
import { PlanTray, type PlanRow } from "./plan-view.tsx";
import { stepAt } from "./plan-state.ts";
import { connectingNote } from "./reconnect-state.ts";
import { fillLog, pinEnd, type Unit, type TranscriptRow } from "./logwindow.tsx";
import { answerHtml, blockedReply } from "./turnrows.ts";
import { stepMark, tickClocks, turnHeading } from "./turn-heading-view.tsx";
import { type AgentEntry, type AgentPage, AG, openModes, openTasks, openOf, agentPics, agentThumbs, AGENT_ACTIVE, agentKind, folderName } from "./agents.ts";
import { continuedFrom, continuedIn, continuedLine } from "./continue-state.ts";
import { stoppingNow } from "./session.tsx";
import { redrawAgentHints } from "./session-head.tsx";
import { promptParts } from "./promptparts-state.ts";
import { childRow } from "./nesting-state.ts";
import { parentSession, sessions } from "./sessions.ts";
import { fillAgentPics } from "./images.tsx";
import { prepareInlineImages } from "./inline-images.ts";
import { picTile } from "./pic-state.ts";
import { canSwitchModel, drawModelPill } from "./modelpill.tsx";
import { modelLabel } from "./agentpick.ts";
import { knownModels, modelsOn } from "./model-store.ts";
import { drawModes } from "./modepill.tsx";
import { backgroundedEntry, drawTasks } from "./tasks.tsx";
import { runs } from "./transcript-cache-state.ts";
import { COMPACTING, waitLine, type BackgroundWork } from "./waiting-state.ts";
import { drawQueue, questionKey } from "./messages.ts";
import { olderNote } from "./older-state.ts";
import type { ChatBody } from "../ui/turn.ts";
import { batchComposerUi, showAgentControl, showAgentControlFor, showAgentStop } from "./composer-ui.tsx";
import { AgentStatus } from "./mode-view.tsx";
import type { BlockedRow, ContextRow, PromptRow, ReplyRow } from "./reply-view.tsx";
import { sessionBody } from "./session-frame.tsx";
import { glide } from "../ui/motion.ts";

/// Draws the log again. `keep` finds the row the reader just opened or
/// folded, which stays where it is on screen. With `older` it is the row in
/// view while older entries come in above, kept only when the reader is not
/// at the end. With `news` only the page's entries changed since the last
/// draw: the turns whose entries stayed keep their rows.
export function drawAgentBody(keep?: string, polled = false, older = false, news = false) {
  const b = sessionBody(), p = AG.page;
  if (!b || !p) return;
  const s = p.session, q = s.question;
  const stopping = stoppingNow();
  if (p.entries.some(subCall)) {
    modelsOn(s.machine_id, s.agent, () => {
      if (AG.page?.session.id === s.id && AG.page.session.machine_id === s.machine_id) drawAgentBody();
    });
  }

  // Read where the reader is before the log is drawn again: only a reader
  // at the bottom follows new entries; anyone else stays where they are,
  // and so does a row the reader opens or folds.
  const redraw = older ? "older" : keep != null ? "toggle" : "news";
  const scroll = logScroll(b.stuck !== false, redraw);
  const anchor = scroll === "keep" ? scrollAnchor(b, keep) : null;
  // A plan waiting for approval shows above the field instead.
  const question = q && openModes()?.plan == null ? q : null;
  const why = AG.why ? `Auto picked ${s.machine_name}. ${AG.why}` : null;
  const olderText = olderNote(p, AG.older.stop, AG.older.reading !== null);
  const parent = parentSession(p.session);
  const crumb: ActivityRow | null = parent ? { kind: "crumb", id: "up", parent: parent.title || "Session", title: s.title || "Session" } : null;
  const olderRow: ActivityRow | null = olderText ? { kind: "older", id: "older", text: olderText } : null;
  // The log goes in by turns, and only those near the view are drawn.
  const before = continuedFrom(s, sessions), after = continuedIn(s, sessions);
  const from: ActivityRow | null = s.continues ? { kind: "continued", id: "continued-from", way: "from", text: continuedLine("from", before, harnessName), open: !!before } : null;
  const into: ActivityRow | null = after ? { kind: "continued", id: "continued-in", way: "in", text: continuedLine("in", after, harnessName), open: true } : null;
  const turns = p.entries.length ? turnUnits(p, news) : [], last = turns.at(-1);
  // The line at the bottom rides with the last turn: a unit of its own after it stalls the window.
  if (into && last) turns[turns.length - 1] = { ...last, rows: () => [...last.rows(), into] };
  const units = p.entries.length
    ? [fixed("up", crumb), fixed("older", olderRow), ...(from ? [fixed("continued-from", from)] : []), ...turns] : null;
  const note: LogShell["note"] = AG.note === "gone" ? { text: "This session is no longer on the machine, or the machine went offline.", tone: "muted" }
    : AG.note && AG.note === AG.readError && AG.link.lost ? { text: AG.note, tone: "muted" }
    : AG.note ? { text: AG.note, tone: "error" }
    : !AG.sig && p.entries.length ? { text: connectingNote(s.machine_name || "the machine"), tone: "connecting" } : null;
  const retry = !!(AG.readError && AG.note === AG.readError && !AG.link.lost && AG.note !== "gone");
  const subs = paneSubs(p);
  const shell = { why, starting: s.status === "starting", log: !!units,
    question: question ? { ...question, options: question.options.map(option => ({ ...option })) } : null,
    questionKey: question ? questionKey(question, openOf(s)) : null,
    answering: !!question && question.id === AG.answering,
    error: p.error, external: s.external, switchModel: !!p.error && canSwitchModel(s), note, retry };
  if (!showLog(b, shell, units, polled)) { AG.rest = ""; return; }
  // The pane follows the open conversation into its worktree.
  followAgent(p.entries, p.earlier, null);
  // The Device tab follows the agent's device calls.
  followDeviceCalls(p.entries, p.earlier);
  const arts = { list: collectArtifacts(p.entries, p.earlier, s.cwd), turnStart: (k: number) => turnStarts.get(k) };
  drawPane(subs, arts);
  batchComposerUi(() => {
    drawModelPill();
    drawModes();
    drawPlan(p);
    drawTasks();
    drawQueue();
    showAgentControl("state", <AgentStatus status={s.status} stopping={stopping} />);
    // Only a turn that runs can be stopped; stopped, it says so at once,
    // while the agent winds the turn down.
    showAgentStop({ shown: AGENT_ACTIVE.has(s.status), stopping });
  });
  tickClocks();
  redrawAgentHints();

  // A log goes to its end as `fillLog` draws it, with one layout.
  if (scroll === "bottom" && !units) pinEnd(b);
  else if (anchor) restoreScroll(b, anchor);
  // A row opened at the bottom pushes the bottom away: the log follows
  // new entries again once the reader scrolls back down.
  if (redraw === "toggle") b.stuck = atBottom(b);
  fillLog(b, scroll === "bottom");
  drawnRows(b);
  syncJump(b);
}

/// Puts the log's html in the page, replacing only the rows that changed.
/// A poll leaves a row the reader is selecting in place until it is
/// unselected: false then, with that row as it was.
function showLog(b: HTMLElement, shell: Parameters<typeof showLogShell>[1], units: Unit[] | null, polled: boolean) {
  const sel = getSelection(), held: Node[] = [];
  const focused = document.activeElement;
  if (polled && focused && b.contains(focused)) held.push(focused);
  if (polled && sel && (!sel.isCollapsed || (pressed !== null && b.contains(pressed)))) {
    if (sel.anchorNode) held.push(sel.anchorNode);
    if (sel.focusNode) held.push(sel.focusNode);
  }
  const prepared = units?.map(unit => ({ ...unit, rows: () => preparedRows(unit.rows()) })) ?? null;
  return showLogShell(b, shell, prepared, held);
}

/// A turn's rows as drawn, worked out once for rows a turn kept (`turnUnits`).
const prepared = new WeakMap<readonly TranscriptRow[], readonly TranscriptRow[]>();
function preparedRows(rows: readonly TranscriptRow[]) {
  let done = prepared.get(rows);
  if (!done) prepared.set(rows, done = rows.map(withInlineImages));
  return done;
}

/// A row with its Markdown images that are files on the session's machine
/// waiting to be read (`fillInlineImages`) rather than loaded by the webview.
function withInlineImages<Row extends TranscriptRow>(row: Row): Row;
function withInlineImages(row: TranscriptRow): TranscriptRow {
  switch (row.kind) {
    case "reply": return { ...row, html: prepareInlineImages(row.html) };
    case "prompt": return { ...row, html: prepareInlineImages(row.html), pictures: prepareInlineImages(row.pictures) };
    case "context": return row.type === "child" ? { ...row, body: prepareInlineImages(row.body) } : row;
    case "thought": return { ...row, content: row.content === null ? null : prepareInlineImages(row.content) };
    case "tool": case "pictures": return { ...row, pictures: prepareInlineImages(row.pictures) };
    case "run": return { ...row, tools: row.tools.map(withInlineImages) };
    default: return row;
  }
}

/// Scrolls the log to its newest entry, which it follows again from there.
export function jumpToLatest() {
  const b = sessionBody();
  if (!b) return;
  b.stuck = true;
  // A glide to the end follows the end as turns measured on the way grow the log.
  glide(b, "end", "glide", { landed: null, done: null });
  syncJump(b);
}

/// The row in view at the top of the log, to pass to `drawAgentBody` as the
/// row that stays where it is when a key opens or folds many rows at once.
export function rowInView() {
  const b = sessionBody();
  return (b && scrollAnchor(b).sel) || "[data-row]";
}

const harnessName = (agent: string) => agentKind(agent).name;

/// A part of the log that is not a turn, as one; an empty one is hidden.
const fixed = (key: string, row: TranscriptRow | null): Unit => ({ key, rows: () => row ? [row] : [], prompt: null });

/// What rows just drawn need: their pictures and their live thoughts.
function drawnRows(b: HTMLElement) {
  fillAgentPics(b);
  attachLive(b);
}

/// The log scrolled: the turns it reaches are drawn. At the bottom it stays there.
export function scrolledLog(b: ChatBody) {
  if (fillLog(b, b.stuck !== false)) drawnRows(b);
}

/// Shows "Scroll to bottom" while the log is scrolled up from the bottom.
export function syncJump(b: ChatBody) {
  const jump = b.querySelector<HTMLElement>("#agjump");
  const hidden = b.stuck !== false || atBottom(b);
  if (jump && jump.hidden !== hidden) jump.hidden = hidden;
}

/// Where a pointer is held down. Matching `:active` on every delta would force
/// a style recalculation of the whole page.
let pressed: Node | null = null;

if (typeof document !== "undefined") {
  document.addEventListener("pointerdown", e => { pressed = e.target instanceof Node ? e.target : null; }, true);
  for (const type of ["pointerup", "pointercancel", "dragend"]) document.addEventListener(type, () => { pressed = null; }, true);
  window.addEventListener("blur", () => { pressed = null; });
}

// Elapsed clocks use when this window first saw the call running.
const toolStarts = new Map<string, number>();

/// A tool entry as one line: its mark and a short label ("Ran cargo test").
function toolData(en: AgentEntry, at: number, pictures: string): ToolActivity | null {
  const parsed = parseTool(imageLines(en.text).text), { title, status } = parsed;
  if (subagent(parsed.output)) return null;
  // A running file send names the bytes it has sent first.
  const { sent, output } = sentFiles(parsed.output);
  const failed = status === "failed", open = stepOpen(failed, at === newestStep, AG.steps.get(at));
  // A command in the background runs on after its turn ended.
  const backgrounded = status === "in_progress" && backgroundedEntry(at);
  const live = status === "in_progress" && (AGENT_ACTIVE.has(AG.page?.session.status) || backgrounded);
  // While the agent asks, the call it holds waits on the answer, not running.
  const waiting = live && !backgrounded && AG.page?.session.status === "asking";
  const label = stepLabel(en.text, live, open);
  const exit = failed ? exitCode(output) : null;
  const beep = beepCall(title);
  const long = title.includes("\n") || label.length > 80;
  const parts = rowParts(en.text, live, open);
  const inspected = !beep && isCommandCall(en.tool) && (!!en.tool?.name || parts.kind === "command");
  const key = `${AG.open?.machine_id ?? ""}\n${AG.open?.session ?? ""}\n${at}`;
  if (live && !toolStarts.has(key)) toolStarts.set(key, Date.now());
  const since = live && !waiting ? toolStarts.get(key) ?? null : null;
  if (!live) toolStarts.delete(key);
  // A command may be cut at any width, so it always opens.
  const opens = inspected || !!output || long || (beep ? !!beep.detail : parts.kind === "command");
  const published = AG.page ? toolArtifacts(en.text, AG.page.session.cwd).files.find(f => f.published) : undefined;
  const state = failed ? (exit != null ? `exit ${exit}` : "Failed") : status === "completed" ? "Done" : waiting ? "Waiting for you" : live ? "Running" : "";
  const keyboard = opens && (inspected || parts.kind === "command");
  // A file the call names opens in the media pane, at most three of them.
  const media = mediaPaths(title).slice(0, 3);
  const common = { sentAt: en.sent_at ?? null, kind: "tool" as const, id: String(at), at, failed, open, artifact: published?.path ?? null, toggle: opens, keyboard, mark: waiting ? "waiting" : stepMark(status, live), media, pictures };

  if (beep) {
    // A Beep call's row shows the first line of its command or prompt; opened, all of it.
    const tail = tailLines(output), whole = AG.all.has(at);
    const more = !tail.cut ? null : whole ? "Show less" : `Show all ${tail.total === 1 ? "of it" : `${tail.total} lines`}`;
    const body: ToolBody | null = open ? { kind: "beep", detail: beep.detail || null, output: output ? { text: whole ? output : tail.shown, whole } : null, more } : null;
    const head = { tip: beepLine(title) ?? title, label: live ? beep.doing : beep.done, machine: beep.machine, line: beep.detail && !open ? beep.detail.split("\n")[0] : null, status: [beep.files, live ? sent : null, state].filter(Boolean).join(" · "), since };
    return { ...common, body, layout: { kind: "beep", head } };
  }

  const wrapped = open && (parts.kind === "command" || long);
  const command = !wrapped ? null : parts.kind === "command" ? { kind: "shell" as const, html: cachedCommandHtml(parts.target) } : { kind: "text" as const, text: parts.target };
  const body: ToolBody | null = !open ? null : inspected ? { kind: "details", details: toolDetails(en.tool, status, live, output) } : { kind: "row", command, output };
  const shown = { ...parts, target: inspected ? parts.target : wrapped ? "" : parts.target };
  return { ...common, body, layout: { kind: "standard", parts: shown, tip: commandOf(title), since } };
}

/// A tool entry's rows: its line, or a subagent's card with the pictures
/// its call showed.
export function toolRows(en: AgentEntry, at: number): ActivityRow[] {
  const { text, names } = imageLines(en.text);
  const pictures = names.map(n => entryPic(n)).join("");
  const tool = toolData({ ...en, text }, at, pictures);
  if (tool) return [tool];
  return [subActivity([at]), ...(pictures ? [{ kind: "pictures" as const, id: `sub:${at}`, at: null, folded: false, pictures }] : [])];
}

/// A subagent's call by session and place, as the window's maps key it.
const subKey = (at: number) => `${AG.open?.session}\n${at}`;

/// What the window saw of each subagent: when it started and ended, by
/// session and the call's place. Calls carry no times.
const subTimes = new Map<string, { start: number; live: boolean; end?: number }>();

/// A subagent's call as its parts: name, how it stands, its steps.
function subOf(at: number) {
  const p = AG.page!, en = p.entries[at - p.earlier];
  const { title, status, output } = parseTool(imageLines(en.text).text);
  const sub = subagent(output) ?? { count: 0, steps: [], result: output, model: null };
  // A linked subagent can keep working after its parent's turn ends.
  const working = sub.child ? sub.child.state === "working" : (status === "in_progress" || status === "pending") && (!!sub.session || AGENT_ACTIVE.has(p.session.status));
  // Timed from when this window saw it working, if it did.
  const key = subKey(at), now = Date.now();
  let seen = subTimes.get(key);
  if (!seen) subTimes.set(key, seen = { start: now, live: working });
  if (!working && seen.live && seen.end == null) seen.end = now;
  const ms = !seen.live ? null : working ? now - seen.start : (seen.end ?? now) - seen.start;
  const [name, ...prompt] = title.split("\n");
  return { title: name, prompt: sub.child ? prompt.join("\n") : "", agent: sub.agent ?? "", status, working, failed: status === "failed", sub, ms, start: seen.start };
}

/// Subagents closed in the pane, by session and the call's place. The log keeps them.
const CLOSED_SUBS_KEY = "closed-subagents";
const CLOSED_SUBS_KEPT = 500;
const closedSubs = new Set<string>();
try { for (const key of JSON.parse(localStorage.getItem(CLOSED_SUBS_KEY) ?? "[]")) closedSubs.add(key); } catch { /* none kept */ }

/// The pane's list: the subagents shown in the log, less those closed.
function subData(at: number, inPane: boolean): SubagentRow {
    const x = subOf(at), kid = x.sub.child;
    const st = kid ? kidState(kid.state) : x.working ? "working" : x.failed ? "failed" : "done";
    const spent = kid?.tokens ? [`${tokenCount(kid.tokens)} tokens`, kid.cost != null ? money(kid.cost) : ""] : [];
    const s = AG.page?.session;
    const model = x.sub.model && s ? modelLabel(knownModels(s.machine_id, s.agent), x.sub.model) : null;
    const meta = [model, st, x.ms != null ? duration(x.ms) : "", kid ? "" : plural(x.sub.count, "step", "steps"), ...spent].filter(Boolean).join(" · ");
    return { at, title: x.title, agent: x.agent, meta, mark: stepMark(x.working ? "in_progress" : x.status, x.working),
      target: kid ? { kind: "child", id: kid.id, key: kid.key } : x.sub.session ? { kind: "native" } : { kind: "summary" },
      dismiss: inPane && st !== "working" && st !== "needs you", stoppable: !!kid && (st === "working" || st === "needs you"),
      result: !kid && !x.sub.session && AG.steps.get(at) ? x.sub.result || "The agent has not provided a readable conversation yet." : null,
      prompt: !inPane && x.prompt ? { text: x.prompt, open: AG.steps.get(at) ?? false } : null };
}

/// The sessions whose plan is opened to its steps.
const openPlans = new Set<string>();

/// The open session's plan above the message field.
function drawPlan(p: AgentPage) {
  const session = AG.open?.session ?? "", plan = planRows(p);
  if (!plan.length) return showAgentControl("steps", null);

  const open = openPlans.has(session), asking = p.session.status === "asking";
  const togglePlan = () => { if (!openPlans.delete(session)) openPlans.add(session); drawAgentBody(); };
  showAgentControlFor("steps", JSON.stringify([session, plan, open, asking]), () => <PlanTray plan={plan} open={open} asking={asking} toggle={togglePlan} toggleStep={toggleStep} />);
}

/// The plan's steps opened to their subagents, by session and the step's text.
const openSteps = new Set<string>();
const stepKey = (content: string) => `${AG.open?.session}\n${content}`;

/// Opens or folds the step at `index` of the open session's plan.
function toggleStep(index: number) {
  const step = AG.page?.plan[index];
  if (!step) return;

  const key = stepKey(step.content);
  if (!openSteps.delete(key)) openSteps.add(key);
  drawAgentBody();
}

/// The plan with the subagents of the entries shown under the step that
/// ran when each started.
function planRows(p: AgentPage): PlanRow[] {
  const under = new Map<number, number[]>();

  for (const at of subagentsShown(p)) {
    const sent = p.entries[at - p.earlier].sent_at;
    const step = sent == null ? null : stepAt(p.plan, sent);
    if (step != null) under.set(step, [...(under.get(step) ?? []), at]);
  }

  return p.plan.map((item, index) => {
    const ats = under.get(index) ?? [];
    const open = ats.length > 0 && openSteps.has(stepKey(item.content));
    return { status: item.status, content: item.content, count: ats.length, subs: open ? ats.map(at => ({ ...subData(at, true), dismiss: false })) : null };
  });
}

function paneSubs(p: AgentPage) {
  const open = subagentsShown(p).filter(at => !closedSubs.has(subKey(at)));
  const rows = open.map(at => subData(at, true));
  return { rows, count: rows.length };
}

/// Takes a finished subagent's row out of the pane.
export function closeSub(at: number) {
  closedSubs.add(subKey(at));
  try { localStorage.setItem(CLOSED_SUBS_KEY, JSON.stringify([...closedSubs].slice(-CLOSED_SUBS_KEPT))); } catch { /* kept for this launch only */ }
  if (AG.page) drawPane(paneSubs(AG.page));
}

/// How an agent another agent started stands, as its card says.
function kidState(state: string) {
  return state === "working" ? "working" : state === "waiting_on_user" ? "needs you" : state === "ended" ? "ended" : "done";
}

/// Subagents started together as one card: "3 subagents · 2 working · 1 done".
function subActivity(ats: number[]): ActivityRow {
  const all = ats.map(subOf), working = all.filter(x => x.working).length, failed = all.filter(x => x.failed).length;
  const done = all.length - working - failed;
  const counts = [working && `${working} working`, done && `${done} done`, failed && `${failed} failed`].filter(Boolean).join(" · ");
  const heading = ats.length > 1 ? `${ats.length} ${all.every(x => x.sub.child) ? "agents started" : "subagents"}` : null;
  return { kind: "subagents", id: String(ats[0]), at: ats[0], heading, counts, rows: ats.map(at => subData(at, false)) };
}

/// How many subagents of the open session's log still work, not counting agents it started.
export function workingSubagents() {
  const p = AG.page;
  return p ? subagentsShown(p).filter(at => { const x = subOf(at); return x.working && !x.sub.child; }).length : 0;
}

/// When this window first saw each background command running, by session and task.
const taskStarts = new Map<string, number>();

/// What the open session runs in the background: its working subagents, not
/// agents it started, and its commands that are not a subagent's.
function backgroundWork(p: AgentPage): BackgroundWork[] {
  const subs = subagentsShown(p), now = Date.now();
  const work: BackgroundWork[] = subs.flatMap(at => {
    const x = subOf(at);
    return x.working && !x.sub.child ? [{ target: { kind: "subagent" as const, at }, title: x.title, start: x.start }] : [];
  });

  for (const task of openTasks()) {
    const key = `${AG.open?.session}\n${task.id}`;
    if (!runs(task)) { taskStarts.delete(key); continue; }
    if (task.entry != null && subs.includes(task.entry)) continue;

    let start = taskStarts.get(key);
    if (start == null) taskStarts.set(key, start = now);
    work.push({ target: { kind: "command" }, title: task.name || task.description, start });
  }

  return work;
}

/// The places of the subagents' calls among the entries shown.
function subagentsShown(p: AgentPage) {
  return p.entries.flatMap((en, i) => subCall(en) ? [p.earlier + i] : []);
}

/// An image a user entry carried: the one sent from here, the one read
/// from the session's machine by its key (an empty tile while it loads),
/// or its name when neither can be.
function entryPic(n: string, original: string | null = null) {
  const key = /^image-([0-9a-f]{16})$/.exec(n)?.[1];
  const sent = (AG.open && agentPics.get(`${AG.open.session}\n${n}`)) || undefined;
  return picTile(n, sent, key ? agentThumbs.get(key) : undefined, original);
}

/// The thought still arriving, by its place, which shows "Thinking"; set
/// by each draw of the turns.
let liveAt: number | null = null;
/// The running turn thinks: a thought streams or is about to.
export let thinking = false;

/// Thoughts start folded because the progress line already shows what
/// the agent thinks. The reader's choice stays as the group grows or ends.
function thoughtRow(en: AgentEntry, at: number, live: boolean): ActivityRow {
  const key = `${AG.open?.session ?? ""}:${at}`;
  const open = !!AG.open && turnsOf(AG.open.session).thoughts.has(at);
  if (live) trackLive(key);
  return { kind: "thought", id: String(at), key, at, text: en.text, content: en.html ?? null, ms: en.ms ?? null, live, open };
}

/// An entry's rows, by its kind.
function entryRows(en: AgentEntry, at: number): TranscriptRow[] {
  if (en.kind === "thought") return [thoughtRow(en, at, at === liveAt)];
  // Codex echoes answered questions as a message: show them as answered.
  const answers = en.kind === "user" ? answeredQuestions(en.text) : null;
  if (answers) return [{ kind: "note", id: `answers:${at}`, at, value: { kind: "answers", items: answers } }];
  if (en.kind === "user") return userRows(en, at);
  if (en.kind === "agent") return agentReply(en, at);
  if (en.kind === "tool") return toolRows(en, at);
  const marker = "[conversation compacted]";
  if (en.kind === "other" && (en.text.trim() === marker || en.text.startsWith(marker + "\n"))) {
    return [{ kind: "note", id: `compacted:${at}`, at, value: { kind: "compacted", summary: en.text.slice(marker.length).trim() || null } }];
  }
  // The files a turn's commands made: one file as "Made x.md", several as a
  // folded "Made 11 files" that opens to their names by kind.
  const files = AG.page ? madeFiles(en.text, AG.page.session.cwd) : null;
  if (files) return [{ kind: "note", id: `made:${at}`, at, value: { kind: "made", files, open: !!AG.open && turnsOf(AG.open.session).made.has(at) } }];
  return [{ kind: "note", id: `other:${at}`, at, value: { kind: "other", text: en.text } }];
}

function userRows(en: AgentEntry, at: number): (PromptRow | ContextRow)[] {
  // Beep's context, what another session sent and what a child came to
  // get rows of their own, never the user's box.
  const parts = promptParts(en.text);
  let child = 0;
  const others = parts.flatMap((p, index): ContextRow[] => p.kind === "context"
    ? [{ kind: "context", id: `${at}:${index}`, type: "context", text: p.text }]
    : p.kind === "session"
      ? [{ kind: "context", id: `${at}:${index}`, type: "session", ...messageSource(p.id, p.name), html: linkified(p.text) }]
      : p.kind === "child"
        ? [((): ContextRow => {
          const row = childRow(sessions, p.id);
          const body = en.child_html?.[child++];
          return { kind: "context", id: `${at}:${index}`, type: "child", part: p, body: body != null ? answerHtml(body) : `<p>${linkified(p.text).replace(/\n/g, "<br>")}</p>`, agent: row?.agent ?? null, ...messageSource(p.id, p.name) };
        })()]
        : []);
  const own = parts.filter(p => p.kind === "user").map(p => p.text).join("\n");
  if (!own && others.length) return others;
  const { names, text: words } = imageLines(own);
  // The user's words come as Markdown HTML, its pasted blocks folded;
  // the ones the reader opened stay open.
  // A message just sent has no HTML until the next read: its text shows meanwhile.
  const html = en.html ?? (words ? words.split(/\n\s*\n/).map(part => `<p>${esc(part).replace(/\n/g, "<br>")}</p>`).join("") : "");
  const said = html.replace(/data-paste="(\d+)"/g, (m, i) => AG.opened.has(`${at}:${i}`) ? `${m} open` : m);
  const attachments = promptAttachments(said);
  const originals = attachmentPictures(attachments.files, names.length);
  const prompt: PromptRow = { sentAt: en.sent_at ?? null, kind: "prompt", row: String(at), html: attachments.html, files: attachments.files.filter(path => !originals.includes(path)), pictures: names.map((name, i) => entryPic(name, originals[i] ?? null)).join("") };
  return [...others, prompt];
}

function messageSource(id: string, name: string): { name: string; project: string | null } {
  const source = childRow(sessions, id);
  return {
    name: source?.title || name,
    project: source ? windowHooks?.sessionProject?.(source)?.label || folderName(source.cwd) || null : null,
  };
}

/// What this window knows of a session's turns: the ones it has seen, when it
/// saw a turn start or end (entries carry no times), the turn running at the
/// last draw, and the tool sections opened to their steps.
interface TurnMemory {
  /// The turns seen, by their place, with the message each starts with.
  known: Map<number, string | null>;
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
  if (!m) turnMemory.set(session, m = { known: new Map(), starts: new Map(), ends: new Map(), running: null, opened: new Set(), thoughts: new Set(), runs: new Set(), made: new Set() });
  return m;
}

/// Notes the turns that started or ended since the last draw. The turns
/// there at the first draw started before this window looked. A log the node
/// wrote again from the start, as when it resumes a session after a restart,
/// holds other turns at the same places: what was noted of the old one goes.
function noteTurns(session: string, turns: Turn[], prompt: (t: Turn) => string | null, running: number | null): TurnMemory {
  const held = turnsOf(session);
  if (turns.some(t => held.known.has(t.key) && held.known.get(t.key) !== prompt(t))) turnMemory.delete(session);

  const m = turnsOf(session), now = Date.now(), first = !m.known.size;
  // A turn older than the ones known was read from further back, not seen starting.
  const newest = Math.max(-1, ...m.known.keys());

  for (const t of turns) {
    if (!m.known.has(t.key) && !first && t.key > newest) m.starts.set(t.key, now);
    m.known.set(t.key, prompt(t));
  }

  if (m.running != null && m.running !== running && !m.ends.has(m.running)) m.ends.set(m.running, now);
  m.running = running;

  return m;
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
  const isSub = (at: number) => subCall(p.entries[at - p.earlier]);
  const flush = () => { if (run.length) out.push(subActivity(run)); run = []; };
  const groups = toolRuns(p.entries, ats, p.earlier, openedSteps());

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
    out.push(...entryRows(en, at));
  }

  flush();
  return out;
}

function agentReply(en: AgentEntry, at: number): (ReplyRow | BlockedRow)[] {
  const { text, names } = imageLines(en.text);
  const pics = names.length ? `<span class="pics">${names.map(n => entryPic(n)).join("")}</span>` : "";
  const linked = (html: string) => markWarnings(linkCodeFiles(linkImagePaths(html).html));
  const html = en.html ?? esc(text);
  const block = blockedReply(text, html);
  const body = block?.html ?? html;
  const replies: (ReplyRow | BlockedRow)[] = body || pics ? [{ kind: "reply", sentAt: en.sent_at ?? null, row: String(at), markdown: !!en.html, html: (en.html ? answerHtml(linked(body)) : linked(body)) + pics, text: (block ? text.slice(0, text.lastIndexOf(block.error)) : text).trim(), copy: false }] : [];
  if (block) replies.push({ kind: "blocked", id: `${at}:blocked`, message: block.error });
  return replies;
}

/// Calls of one kind in a row as one line, the first few targets beside
/// it; it opens in place, and stays open across redraws.
function runRow(p: AgentPage, ats: number[]): ActivityRow {
  const en = (at: number) => p.entries[at - p.earlier];
  const kind = toolKind(en(ats[0]).text);
  const names = ats.map(at => rowParts(en(at).text).target.split("/").pop() || "").filter(Boolean);
  const shown = names.slice(0, 3).join(", ") + (names.length > 3 ? ` and ${names.length - 3} more` : "");
  const open = !!AG.open && turnsOf(AG.open.session).runs.has(ats[0]);
  const tools = ats.flatMap(at => toolRows(en(at), at));
  return { kind: "run", id: String(ats[0]), at: ats[0], toolKind: kind, label: runLabel(kind, ats.length), summary: shown, open, tools } satisfies ActivityRow;
}

/// The rows of the finished turns last drawn, by turn, and what they were
/// drawn with; anything else changed and they are worked out again.
interface KeptRows {
  scope: { session: string; status: string; earlier: number; sessions: typeof sessions };
  turns: Map<number, { entries: AgentEntry[]; texts: string[]; rows: readonly TranscriptRow[] }>;
}
let keptRows: KeptRows | null = null;

/// A tool entry's parts, read once per entry and text; null for other entries.
const toolPartsRead = new WeakMap<AgentEntry, { text: string; parts: ReturnType<typeof parseTool>; sub: boolean }>();
function toolRead(en: AgentEntry) {
  if (en.kind !== "tool") return null;
  let read = toolPartsRead.get(en);

  if (read?.text !== en.text) {
    const parts = parseTool(en.text);
    read = { text: en.text, parts, sub: !!subagent(parts.output) };
    toolPartsRead.set(en, read);
  }

  return read;
}
const toolParts = (en: AgentEntry) => toolRead(en)?.parts ?? null;
/// Whether a tool entry started a subagent.
const subCall = (en: AgentEntry) => toolRead(en)?.sub ?? false;

/// The log in turns: each message, then a heading with how long the agent
/// worked, then its work and its answer. A turn's rows are worked out only
/// when it is drawn (logwindow.ts). With `news` a finished turn before the
/// last whose entries stayed keeps the rows it had.
function turnUnits(p: AgentPage, news: boolean): Unit[] {
  const s = p.session, active = AGENT_ACTIVE.has(s.status);
  const scope = { session: AG.open?.session ?? s.id, status: s.status, earlier: p.earlier, sessions };
  const same = news && keptRows && (Object.keys(scope) as (keyof typeof scope)[]).every(k => keptRows?.scope[k] === scope[k]);
  const kept: KeptRows = same && keptRows ? keptRows : keptRows = { scope, turns: new Map() };
  newestStep = null;
  thinking = false;
  turnStarts.clear();
  const turns = splitTurns(p.entries, p.earlier), lastTurn = turns.at(-1);
  const rows = (at: number) => entryRows(p.entries[at - p.earlier], at);
  const entry = (at: number) => p.entries[at - p.earlier];
  const prompt = (t: Turn) => { const said = t.user == null ? null : entry(t.user); return said?.kind === "user" && said.text.trim() ? said.text.trim() : null; };
  const m = noteTurns(AG.open?.session ?? s.id, turns, prompt, active && lastTurn ? lastTurn.key : null);
  liveAt = lastTurn ? liveThought(p.entries, lastTurn, active, p.earlier) : null;
  return turns.map(t => ({ key: `t${t.key}`, rows: t === lastTurn ? turnRows(t) : keptTurn(t, turnRows(t)), prompt: prompt(t) }));

  /// The rows of a turn before the last: those it had while its entries stay, once none runs.
  function keptTurn(t: Turn, rows: () => TranscriptRow[]): () => readonly TranscriptRow[] {
    return () => {
      const entries = [...(t.user == null ? [] : [t.user]), ...t.steps, ...t.answer].map(entry);
      const held = kept.turns.get(t.key);
      if (held && held.entries.length === entries.length && held.entries.every((en, i) => en === entries[i] && held.texts[i] === en.text)) return held.rows;
      const made = rows();
      // A call that runs, or a subagent, shows a clock or its state from outside its entry.
      const settled = entries.every(en => { const read = toolRead(en); return !read || (!read.sub && read.parts.status !== "in_progress" && read.parts.status !== "pending"); });
      if (settled) kept.turns.set(t.key, { entries, texts: entries.map(en => en.text), rows: made });
      return made;
    };
  }

  function turnRows(t: Turn): () => TranscriptRow[] {
    // Notes before the first message are not a turn of work.
    if (onlyNotes(p.entries, t, p.earlier)) return () => t.steps.flatMap(rows);
    const last = t === lastTurn, running = active && last;
    const user = (): TranscriptRow[] => {
      if (t.user == null) return [];
      const item = entry(t.user);
      return item.kind === "user" && !answeredQuestions(item.text) ? userRows(item, t.user) : rows(t.user);
    };
    if (!running && !t.steps.length && !t.answer.length) return user;
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
          && (running || toolParts(entry(latestTool))?.status === "failed") ? latestTool : null;
        if (preview != null) kept.add(preview);
        const folding = section.ats.filter(at => !kept.has(at));
        const stats = progressSummary(p.entries, group, p.earlier);
        const head = turnHeading({ state: "working", start: null, ms: null, stats, turn: key, opened });
        const seen = opened ? [] : folding.flatMap(at => imageLines(entry(at).text).names);
        const pictures = seen.map(n => entryPic(n)).join("");
        const pics: ActivityRow | null = seen.length ? { kind: "pictures", id: String(folding[0]), at: folding[0], folded: true, pictures } : null;
        const shownRows = rowsOf(p, opened ? section.ats : section.ats.filter(at => kept.has(at)));
        const collapse: ActivityRow | null = opened ? { kind: "note", id: `collapse:${key}`, at: key, value: { kind: "collapse" } } : null;
        return [...(head ? [head] : []), ...(pics ? [pics] : []), ...shownRows, ...(collapse ? [collapse] : [])];
      });
      const reply = work.findLast(row => row.kind === "reply" && row.text);
      if (reply?.kind === "reply") reply.copy = true;

      const head = turnHeading({ state, start: start ?? null, ms, stats: "", turn: null, opened: false });
      const shownTitle = newest != null ? toolParts(entry(newest))?.title.trim() : "";
      const liveDoing = state === "working" && doing !== "Thinking" && !(shownTitle && (shownTitle.includes(doing) || doing.includes(shownTitle))) ? doing : "";
      // Replied, but subagents or commands in the background hold the turn open.
      // A compaction the node started after the reply says so instead.
      const wait = state === "working" && t.answer.length && status !== COMPACTING ? waitLine(backgroundWork(p)) : null;
      const live: ActivityRow | null = !running ? null : wait ? { kind: "waiting", id: String(t.key), line: wait } : { kind: "live", id: String(t.key), state, thinking, start: start ?? null, doing: liveDoing };
      return [...user(), ...(head ? [head] : []), ...work, ...(live ? [live] : [])];
    };
  }
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

/// The first line of a thought, without its Markdown bold or heading marks.
function thoughtLine(text: string) {
  const line = text.split("\n").map(l => l.trim()).find(Boolean) ?? "";
  return line.replace(/^#+\s*/, "").replace(/\*\*/g, "").trim();
}
