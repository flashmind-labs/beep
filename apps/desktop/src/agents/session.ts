// One coding agent session in the bar: opening it, following it while it
// shows, its frame, and what a click in it does.
import { markDelivered } from "./agentqueue.ts";
import { composeField } from "../chat/compose.ts";
import { $, $$, type BackTo, bar, PANE, S, windowHooks } from "../bar/core.ts";
import { hints } from "../bar/hints.ts";
import { openViewer } from "../chat/media.ts";
import { linkedPath } from "../agentpane/media-state.ts";
import { openLinked, openPath, openPicture, paneFrame, showArtifact, startPane } from "../agentpane/media.ts";
import { atBottom } from "./logscroll.ts";
import { copyCode, stopFace, toggleThought } from "./turnrows.ts";
import { MN, resetMentions } from "./workdrop.ts";
import { type AgentRow, type AgentPage, type AgentUsage, type OpenAgent, AG, usageLine, AGENT_POLL_MS, AGENT_THINK_POLL_MS, AGENT_STOP_POLL_MS, AGENT_SLOW_AFTER, AGENT_SLOW_MS, AGENT_PAUSE_AFTER, AGENT_ACTIVE, AGENT_LAST, agentKind, canTakeOver, agentAvatar, folderName, where, stateTag } from "./agents.ts";
import { agentAttachError, agentAttach, drawAgentAttached, fullPic } from "./images.ts";
import { closeChild, thinking, turnsOf, clickedRow, drawAgentBody, syncJump, jumpToLatest } from "./logview.ts";
import { drawModelPill, toggleModelPicker, closeModelPicker } from "./modelpill.ts";
import { cycleMode, hasModes, readModes } from "./modepill.ts";
import { readTasks, stopTask } from "./tasks.ts";
import { editorTools } from "../project/editors.ts";
import { editQueued, flushQueue, interruptAgent, answerAgentQuestion } from "./messages.ts";
import type { ChatBody } from "../ui/turn.ts";
import { esc } from "../ui/format.ts";
import { h } from "../ui/dom.ts";
import { ICON } from "../ui/icons.ts";
import { invoke, listen } from "../ui/tauri.ts";

/// Opens session `s` (a session row) and follows it while it is on screen.
export function openAgent(s: AgentRow, { why = "", back: from }: { why?: string; back?: string | BackTo } = {}) {
  if (from === undefined) from = S.view === "agents" ? "agents" : "chat";
  S.view = "agent";
  Object.assign(AG, {
    open: { machine_id: s.machine_id, agent: s.agent, session: s.id },
    page: { session: s, plan: [], files: [], entries: [], earlier: 0, last_message: "", error: null, queued: 0, turns: 0 },
    why, back: from, note: "", sig: "", changed: Date.now(), paused: false, answering: null,
    att: [], attError: "", noImages: null, opened: new Set(), steps: new Map(), all: new Set(), child: null, queue: [], flushing: false, stopping: null, modelOpen: false, modes: null,
  });
  drawAgentFrame();
  pollAgent();
}

export const isOpen = (o: OpenAgent | null) => S.view === "agent" && AG.open && o === AG.open;

/// Whether the session view can be seen: the window is not hidden, and
/// the bar, when this is the bar, is up.
export const agentOnScreen = () => document.visibilityState === "visible" && !AG.barHidden;

/// How long until the open session is asked again, or null to stop: an
/// ended session, or a quiet one that has not changed for a while.
export function agentDelay() {
  const s = AG.page?.session;
  if (!s || s.status === "ended") return null;
  if (thinking && AGENT_ACTIVE.has(s.status)) return AGENT_THINK_POLL_MS;
  // Once stopped, the turn ending shows as soon as it does.
  if (!s.external && AGENT_ACTIVE.has(s.status)) return stoppingNow() ? AGENT_STOP_POLL_MS : AGENT_POLL_MS;
  const still = Date.now() - AG.changed;
  if (still >= AGENT_PAUSE_AFTER) return null;
  return still >= AGENT_SLOW_AFTER ? AGENT_SLOW_MS : AGENT_POLL_MS;
}

/// Whether the reader is selecting text in the log or has some selected. A
/// redraw replaces the rows, which loses where a selection started, so a poll
/// waits and draws once the selection is gone.
function selecting() {
  const log = $("#agbody");
  if (!log) return false;
  if (log.matches(":active")) return true;
  const selection = getSelection();
  return !!selection && !selection.isCollapsed && log.contains(selection.anchorNode);
}

/// Asks for the open session, then again after `agentDelay()` while it
/// stays on screen. Never two calls at once for one session.
export async function pollAgent() {
  clearTimeout(AG.timer);
  const o = AG.open;
  if (!o || !isOpen(o) || AG.inflight === o || !agentOnScreen()) return;
  AG.inflight = o;
  const read = ++AG.polls;
  try {
    const [page, modes, tasks] = await Promise.all([invoke<AgentPage>("agent_view", { ...where(o), last: AGENT_LAST }), readModes(o), readTasks(o)]);
    if (isOpen(o)) {
      AG.modes = modes;
      AG.tasks = tasks;
      const sig = JSON.stringify(page);
      if (sig !== AG.sig) { AG.sig = sig; AG.changed = Date.now(); }
      if (page.session.question?.id !== AG.answering) AG.answering = null;
      // A session opened with no folder (from a notice) draws again once a read gives it.
      const placed = !AG.page?.session.cwd && !!page.session.cwd;
      AG.page = page;
      if (placed) drawAgentFrame();
      // What the last failed read said no longer holds.
      if (AG.note === "gone" || AG.note === AG.readError) AG.note = "";
      AG.readError = "";
      if (!AGENT_ACTIVE.has(page.session.status)) AG.stopping = null;
      markDelivered(AG.queue, page.queued, read);
      if (!selecting()) drawAgentBody();
      void readUsage(o, page);
      flushQueue();
    }
  } catch (e) {
    if (isOpen(o)) {
      // A dropped connection comes back by itself; the next read says so.
      AG.readError = /not found/i.test(String(e)) ? "" : /connection lost|closed by peer|timed out/i.test(String(e)) ? "Reconnecting to Beep…" : String(e);
      AG.note = AG.readError || "gone";
      drawAgentBody();
    }
  } finally {
    if (AG.inflight === o) AG.inflight = null;
  }
  // Another session opened while this one was asked: follow that one.
  if (AG.open !== o) { if (isOpen(AG.open) && !AG.inflight) pollAgent(); return; }
  if (!isOpen(o) || !agentOnScreen()) return;
  const delay = agentDelay();
  const paused = delay === null && AG.page?.session.status !== "ended";
  if (paused !== AG.paused) { AG.paused = paused; drawAgentBody(); }
  if (delay !== null) AG.timer = setTimeout(pollAgent, delay);
}

/// Follows the open session again after a pause or a message.
export function resumeAgent() {
  AG.changed = Date.now();
  AG.paused = false;
  pollAgent();
}

document.addEventListener("visibilitychange", () => {
  if (document.visibilityState === "visible") { if (S.view === "agent") pollAgent(); } else clearTimeout(AG.timer);
});
if (!PANE) {
  listen("bar-hidden", () => { AG.barHidden = true; clearTimeout(AG.timer); });
  listen("bar-shown", () => { AG.barHidden = false; if (S.view === "agent") pollAgent(); });
}

/// Whether the server turned images down for the open session.
export const noAgentImages = () => AG.noImages != null && AG.noImages === AG.open?.session;

/// Whether the open session's turn runs, so Enter steers, Option-Enter
/// queues and ⌘. stops.
export const turnRuns = () => {
  const s = AG.page?.session;
  return !!s && !s.external && !s.question && AGENT_ACTIVE.has(s.status);
};

/// Whether the user stopped the open session's turn and it has not ended.
export const stoppingNow = () => {
  const s = AG.page?.session;
  return !!s && AG.stopping === s.id && AGENT_ACTIVE.has(s.status);
};

/// The session's keys on one small line.
export function agentHints() {
  const s = AG.page?.session, q = s?.question;
  const slim = (items: Parameters<typeof hints>[0]) => hints(items).replace('class="dhints"', 'class="dhints slim"');
  const mode: Parameters<typeof hints>[0] = hasModes() ? [["⇧Tab", "mode"]] : [];
  if (q && AG.modes?.plan != null) return slim([[`⌘${ICON.ret}`, "approve"], ["⌘D", "keep planning"], ...mode, ["esc", "back"]]);
  if (q) return slim([[`⌘${ICON.ret}`, "allow"], ["⌘D", "deny"], ["esc", "back"]]);
  if (AG.child != null) return slim([["esc", "parent session"]]);
  if (s?.external) return slim([["esc", "back"]]);
  if (s?.status === "ended") return slim([[ICON.ret, "continue"], ["esc", "back"]]);
  if (turnRuns()) {
    const keys: Parameters<typeof hints>[0] = [[ICON.ret, "steer"], [`⌥${ICON.ret}`, "queue"]];
    if (stoppingNow()) return slim(keys);
    return slim([...keys, ...mode, ["⌘.", AG.queue.length ? "stop and send queued" : "stop"], ["esc", "back"]]);
  }
  // Once one is attached its chip says enough.
  const paste = !AG.att.length && !noAgentImages() ? [["⌘V", "paste an image"]] : [];
  return slim([...paste, [ICON.ret, "send"], ...mode, ["⌘T", "thoughts"], ["⌘Y", "commands"], ["esc", "back"]]);
}

/// Puts the session's key hints in place of those on screen.
export function redrawAgentHints() {
  const hint = $("#agconv > .dhints");
  if (hint) hint.replaceWith(h(agentHints()));
}

/// A session's folder as the header shows it: the repository and its
/// worktree (`widget · fix-login`), else the folder's name.
export function shortCwd(cwd: string) {
  const m = /^(.*?)\/(?:\.claude\/worktrees|\.worktrees)\/(.+?)\/?$/.exec(cwd);
  if (m) return `${folderName(m[1])} · ${m[2]}`;
  return folderName(cwd) || cwd;
}

/// The harnesses that switch models in a running session.
export const SWITCHES = new Set(["claude", "codex", "opencode"]);
/// The model and thinking switched to in each session from here, by session.
export const sessionPicks = new Map<string, { model: string; effort: string | null }>();

/// The session the mentions waiting above the field belong to.
let paneSessionKey = "";

/// The head's usage line, the session it is of, and the turns it counts.
let usage = { of: "", turns: -1, text: "" };
const usageKey = (s: AgentRow) => `${s.machine_id}\n${s.agent}\n${s.id}`;

/// Reads the open session's usage once per finished turn into the head.
async function readUsage(o: OpenAgent, page: AgentPage) {
  const s = page.session, of = usageKey(s);
  if (s.external || (usage.of === of && usage.turns === page.turns)) return;
  usage = { of, turns: page.turns, text: usage.of === of ? usage.text : "" };
  try {
    const u = await invoke<AgentUsage | null>("agent_usage", where(o));
    if (usage.of !== of || !u) return;
    usage.text = usageLine(u);
    const line = $("#aguse");
    if (line && isOpen(o)) line.textContent = usage.text;
  } catch (e) {
    console.warn("agent usage not read", e);
    if (usage.of === of) usage.turns = -1;
  }
}

/// Open in an editor, in the head, when this Mac has the session's folder.
async function drawEditors(s: AgentRow) {
  const folder = await invoke<string | null>("session_folder_here", { machineId: s.machine_id, cwd: s.cwd })
    .catch(e => { console.warn("session folder not read", e); return null; });
  const draw = () => {
    if (AG.page && usageKey(AG.page.session) === usageKey(s)) $("#agedit")?.replaceChildren(...editorTools(folder, draw));
  };
  draw();
}

/// The parts that stay while the session updates: the head, the body and the field.
export function drawAgentFrame() {
  const s = AG.page!.session, k = agentKind(s.agent);
  const field = s.external
    ? `<div class="dmuted dblock dbanner">Started outside Beep. Beep follows it here${canTakeOver(s.agent) ? "; to steer it, ask Beep in a chat to take it over once it stops" : ""}.</div>`
    : `<div id="agplan"></div><div id="agtasks"></div><div class="dinput small" id="agfield"><textarea id="agmsg" rows="1" autocomplete="off" spellcheck="false" placeholder="Message ${esc(k.name)}"></textarea><span id="agmode"></span><span id="agmodel"></span><button class="dbtn" id="agstop" aria-label="Stop the turn" title="Stop the turn (⌘.)">${stopFace()}</button></div><div id="agpickm"></div>`;
  // The conversation fills the window; the media pane sits beside it or under it.
  const project = windowHooks?.sessionProject?.(s);
  bar.innerHTML = `<div class="dhead">${agentAvatar(s.agent)}<span class="tt"><b>${project ? `<button class="agproj" id="agproj">${esc(project.label)}</button>` : esc(shortCwd(s.cwd) || k.name)}</b><small title="${esc(s.cwd)}">${esc(s.machine_name)}</small></span><small id="aguse" class="aguse">${esc(usage.of === usageKey(s) ? usage.text : "")}</small><span id="agstate">${stateTag(s.status)}</span><span id="agedit" class="agedit"></span></div>
    <div class="agshell" id="agshell">${paneFrame(`<div class="agconv" id="agconv"><div class="dbody" id="agbody"></div>${field}${agentHints()}</div>`)}</div>`;
  if (paneSessionKey !== `${s.machine_id}\n${s.id}`) { paneSessionKey = `${s.machine_id}\n${s.id}`; resetMentions(); }
  if (project) $("#agproj")?.addEventListener("click", project.open);
  MN.redraw = drawAgentAttached;
  MN.error = agentAttachError;
  startPane(s);
  void drawEditors(s);
  const msg = $<HTMLTextAreaElement>("#agmsg");
  if (msg) composeField(msg, { images: agentAttach, grow: 156 });
  drawAgentAttached();
  drawModelPill();
  const b = $$<ChatBody>("#agbody");
  b.stuck = true;
  b.onscroll = () => { b.stuck = atBottom(b); syncJump(b); };
  drawAgentBody();
  $("#agmsg")?.focus();
  bar.onclick = agentClick;
}

function agentClick(e: MouseEvent) {
  if (S.view !== "agent") return;
  const target = e.target as HTMLElement;
  const opt = target.closest<HTMLElement>("[data-agopt]");
  if (opt) return answerAgentQuestion(Number(opt.dataset.agopt));
  if (target.closest("#agmodelb")) return toggleModelPicker();
  if (target.closest("#agmodeb")) { cycleMode(); $("#agmsg")?.focus(); return; }
  const task = target.closest<HTMLElement>("[data-agtask]");
  if (task) return void stopTask(task.dataset.agtask!);
  if (AG.modelOpen && !target.closest(".agc-pick")) closeModelPicker();
  const queued = target.closest<HTMLElement>("[data-agq]");
  if (queued) return void editQueued(Number(queued.dataset.agq), queued.dataset.act === "edit");
  // A subagent opens as its own conversation; the crumb goes back.
  const sub = target.closest<HTMLElement>(":is(#agbody, .agpane) [data-sub]");
  if (sub) { AG.child = Number(sub.dataset.sub); drawAgentBody(); $("#agbody")!.scrollTop = 0; return; }
  if (target.closest("#agparent")) return closeChild();
  // "Show all N lines" opens the whole output in a box that scrolls.
  const all = target.closest<HTMLElement>("#agbody [data-all]");
  if (all) {
    const at = Number(all.dataset.all);
    if (!AG.all.delete(at)) AG.all.add(at);
    return drawAgentBody(clickedRow(all));
  }
  if (target.closest("#agbody pre.out.all")) return;
  // A run of calls opens and folds in place, like a thought.
  const runBtn = target.closest<HTMLElement>("#agbody [data-run]");
  if (runBtn && AG.open) {
    const opened = turnsOf(AG.open.session).runs, at = Number(runBtn.dataset.run);
    const open = !opened.delete(at);
    if (open) opened.add(at);
    runBtn.parentElement?.classList.toggle("open", open);
    runBtn.setAttribute("aria-expanded", String(open));
    return;
  }
  const copy = target.closest<HTMLElement>("#agbody [data-copy]");
  if (copy) return copyCode(copy);
  if (target.closest("#agjump")) { jumpToLatest(); return; }
  // A thought opens and folds in place, so only it animates.
  const thought = target.closest<HTMLElement>("#agbody [data-thought]");
  if (thought && AG.open) {
    const m = turnsOf(AG.open.session), at = Number(thought.dataset.thought);
    // A thought streams open; once the reader folds or opens it, it keeps
    // their choice when it ends.
    m.touched.add(at);
    if (toggleThought(thought)) m.thoughts.add(at);
    else m.thoughts.delete(at);
    return;
  }
  // A finished turn's heading or folded line opens its steps; the heading folds them again.
  const turn = target.closest<HTMLElement>("#agbody [data-turn]");
  if (turn && AG.open) {
    const opened = turnsOf(AG.open.session).opened, key = Number(turn.dataset.turn);
    if (!opened.delete(key)) opened.add(key);
    return drawAgentBody(clickedRow(turn));
  }
  // A file a step names opens in the media pane.
  const step = target.closest<HTMLElement>("#agbody [data-media]");
  if (step) { openPath(step.dataset.media!); return; }
  // A markdown link to a file opens it in the pane; bar/events.ts sends web links out.
  const fileLink = target.closest<HTMLAnchorElement>("#agbody a[href]");
  const linkedFile = fileLink && linkedPath(fileLink.getAttribute("href")!);
  if (linkedFile) { openLinked(linkedFile); return; }
  // A file the reply names opens its picture, as its thumbnail does, or
  // else the file itself from the machine, in the media pane.
  const link = target.closest<HTMLElement>("#agbody a.imgl");
  const linked = link?.dataset.img != null ? link.closest(".dcoderen")?.querySelectorAll<HTMLImageElement>(":scope > .pics img.pic")[Number(link.dataset.img)] : null;
  if (link && !linked?.getAttribute("src")) { if (link.dataset.path) openPath(link.dataset.path); return; }
  // The agent's pictures open in the media pane, the user's own full size.
  // One still loading opens once it is here. The tile may be a thumbnail, so
  // what opens is read whole.
  const pic = linked ?? target.closest<HTMLImageElement>("#agbody img.pic");
  if (pic) {
    const o = AG.open;
    if (!o || !pic.getAttribute("src")) return;
    const name = (link ? link.textContent : null) || pic.dataset.name || pic.alt;
    const unread = () => { AG.note = "Could not read the whole picture."; drawAgentBody(); };
    if (!pic.closest(".dcoderen.user")) {
      fullPic(o, pic).then(url => { if (isOpen(o)) url ? openPicture(url, name) : unread(); });
      return;
    }
    const all = [...pic.parentElement!.querySelectorAll<HTMLImageElement>("img.pic[src]")];
    Promise.all(all.map(i => fullPic(o, i))).then(urls => {
      if (!isOpen(o)) return;
      // Each opens by its data URL, which can be saved or copied.
      const read = all.flatMap((i, at) => { const url = urls[at]; return url ? [{ img: i, url }] : []; });
      const at = read.findIndex(r => r.img === pic);
      if (at < 0) return unread();
      openViewer(read.map(({ img: i, url }) => ({ url, name: i.dataset.name || i.alt, caption: "", saves: url.startsWith("data:") })), at);
    });
    return;
  }
  // An Artifact call opens what it published in the Artifacts tab.
  const art = target.closest<HTMLElement>("#agbody [data-artifact]");
  if (art && !getSelection()!.toString() && showArtifact(art.dataset.artifact!)) return;
  const tool = target.closest<HTMLElement>("#agbody .dcoderen.tool[data-at]");
  if (tool && !getSelection()!.toString() && !target.closest(".out, .ddiff, .cmd")) {
    const at = Number(tool.dataset.at);
    AG.steps.set(at, !tool.classList.contains("open"));
    return drawAgentBody(clickedRow(tool));
  }
  // A pasted block opens by itself; remember it so a redraw keeps it open.
  const paste = target.closest<HTMLElement>("#agbody .dcoderen.user[data-at] details.paste > summary");
  if (paste) {
    const key = `${paste.closest<HTMLElement>("[data-at]")!.dataset.at}:${paste.parentElement!.dataset.paste}`;
    if (!AG.opened.delete(key)) AG.opened.add(key);
    // Once it opened, the next redraw follows the bottom only if it is still in view.
    requestAnimationFrame(() => { const b = $<ChatBody>("#agbody"); if (b) b.stuck = atBottom(b); });
    return;
  }
  if (target.closest("#agparent2")) return closeChild();
  if (target.closest("#agresume")) return resumeAgent();
  if (target.closest("#agstop")) return void interruptAgent();
}

/// A user entry's text and the images it carried: the node writes each
/// as a line `[image: name]` after the text.
export function userImages(text: string) {
  const lines = String(text).split("\n"), names: string[] = [];
  while (lines.length) {
    const m = /^\[image: (.+)\]$/.exec(lines[lines.length - 1]);
    if (!m) break;
    names.unshift(m[1]);
    lines.pop();
  }
  return { text: lines.join("\n"), names };
}
