import { SessionPicks } from "./agentmodels.ts";
import { bindSessionStash } from "./stash.ts";
import { stashIndicator, stashActions } from "../chat/stash.ts";
import { AgentFeed } from "./agentfeed-state.ts";
import { TranscriptCache, transcriptKey, type TranscriptUpdate } from "./transcript-cache-state.ts";
import { agentDrop } from "../chat/attach.ts";
import { filePicker } from "./file-picker.ts";
import { fileLinkMenu } from "./filelinks.ts";
import { bindTitleEdit } from "./titleedit.ts";
import { conversationHistory } from "./conversation-state.ts";
// One coding agent session in the bar: opening it, following it while it
// shows, its frame, and what a click in it does.
import { syncSteers, stopPending, type Waiting } from "./agentqueue.ts";
import { composeField } from "../chat/compose.ts";
import { $, $$, type AgentReturn, bar, S, windowHooks, changeListeners } from "../bar/core.ts";
import { failBanner } from "../ui/failbanner.ts";
import { hints } from "../bar/hints.ts";
import { SHORTCUTS } from "../ui/shortcuts.ts";
import { openViewer } from "../chat/media.ts";
import { readRunning } from "../agentpane/running.ts";
import { linkedPath } from "../agentpane/media-state.ts";
import { openLinked, openPath, openPicture, paneFrame, rebindPane, showArtifact, startPane } from "../agentpane/media.ts";
import { atBottom, followsAfterScroll, returnScroll } from "./logscroll.ts";
import { copyCode, stopFace, toggleThought } from "./turnrows.ts";
import { bindMentionMenu } from "./mentionmenu.ts";
import { MN, resetMentions } from "./workdrop.ts";
import { type AgentRow, type AgentPage, type AgentUsage, type OpenAgent, AG, drawAgents, usageLine, AGENT_POLL_MS, AGENT_THINK_POLL_MS, AGENT_STOP_POLL_MS, AGENT_SLOW_AFTER, AGENT_SLOW_MS, AGENT_ACTIVE, AGENT_LAST, AGENT_READ_WAIT_MS, agentKind, agentBack, canTakeOver, agentAvatar, folderName, where, stateTag } from "./agents.ts";
import { agentAttachError, agentAttach, drawAgentAttached, fullPic } from "./images.ts";
import { childRow } from "./nesting-state.ts";
import { parentSession, keepNativeSession, loadSessions, sessions, sessionsRead } from "./sessions.ts";
import { closeSession, shownStatus } from "./closing.ts";
import { sessionOffers } from "./sessionmenu-state.ts";
import { thinking, turnsOf, clickedRow, drawAgentBody, syncJump, jumpToLatest, closeSub, scrolledLog } from "./logview.ts";
import { drawModelPill, toggleModelPicker, closeModelPicker } from "./modelpill.ts";
import { cycleMode, hasModes, readModes } from "./modepill.ts";
import { readTasks, stopTask } from "./tasks.ts";
import { editorTools, openInEditor } from "../project/editors.ts";
import { editQueued, interruptAgent, answerAgentQuestion, sendAgentMessage, steerMessage } from "./messages.ts";
import type { ChatBody } from "../ui/turn.ts";
import { esc } from "../ui/format.ts";
import { h } from "../ui/dom.ts";
import { ICON } from "../ui/icons.ts";
import { Channel, invoke, listen, logError, logInfo, recordTiming } from "../ui/tauri.ts";
import { onShownChange, paneShown, visibilitySnapshot } from "../ui/active.ts";
import { readError } from "../agentpane/read.ts";
import { loadOlder, nearTop } from "./older.ts";
import { joinRuns, lacksPrompt, newestRead } from "./older-state.ts";
import { endedPage, hasNews, LINKED, noteAfterRead, readFailed, retryDelay } from "./reconnect-state.ts";
import { onPlaces, placeOf } from "./place.ts";
import { drawMove } from "./move.ts";
import { MV, refreshMoves } from "./move-store.ts";
import { movedTo } from "./move-state.ts";
import { placeName, placeRepo } from "./place-state.ts";
import { sameSession } from "./running-state.ts";
import { imageLines } from "./agentlog.ts";
import { isNativeSession, nativeSession, nativeUpdate } from "./native-session-state.ts";

let firstRead: { open: OpenAgent; at: number } | null = null;
const steerQueues = new Map<string, Waiting[]>();
function finishFirstRead(open: OpenAgent, outcome: "ok" | "error") {
  if (!firstRead || firstRead.open !== open) return;
  recordTiming("data", "agent", performance.now() - firstRead.at, outcome);
  firstRead = null;
}

/// Opens session `s` (a session row) and follows it while it is on screen. With `adopt`,
/// the empty view that started it becomes the session in place: its pane, terminal
/// tabs included, stays as it is.
export function openAgent(s: AgentRow, { why = "", back: from, adopt = false }: { why?: string; back?: AgentReturn; adopt?: boolean } = {}) {
  conversationHistory.visit(s);
  traceSession("opening", { machine_id: s.machine_id, agent: s.agent, session: s.id }, { status: s.status, adopt });

  // Already open: keep its log, draft, queue and scroll, and only ask for news.
  if (!why && sameSession(AG.open, s) && AG.page?.session.id === s.id) {
    if (AG.page.session.status === "ended" && s.status !== "ended") {
      stopAgentFollow();
      AG.page.session = s;
      AG.sig = "";
      drawAgentHeading();
      drawModelPill();
      redrawAgentHints();
    }
    if (AG.open) firstRead = { open: AG.open, at: performance.now() };
    if (from !== undefined) AG.back = from;
    // Its log may come back from a hidden page, which WebKit shows scrolled
    // to the top: it goes back to its end, or where the reader left it,
    // before its scroll handler takes that for the reader's move.
    const shown = $<ChatBody>("#agbody");

    if (S.view === "agent" && shown) {
      shown.scrollTop = returnScroll(shown, shown.top ?? 0);
      return void pollAgent(true);
    }

    if (from === undefined) AG.back = S.view === "agents" ? "agents" : "chat";
    S.view = "agent";
    Object.assign(AG, { sig: "", rest: "" });
    // A log the reader scrolled up in goes back where they left it; one at its end starts there again.
    drawAgentFrame(false, left?.key === sessionKey(s) && left.body.stuck === false ? left.body : null);
    watchRead(AG.open!);
    return void pollAgent(true);
  }

  if (from === undefined) from = S.view === "agents" ? "agents" : "chat";
  const inPlace = adopt && !!AG.draft && S.view === "agent" && !!$("#agshell");
  if (AG.open) steerQueues.set(transcriptKey(AG.open), AG.queue);
  for (const [key, queue] of steerQueues) if (!queue.length) steerQueues.delete(key);
  S.view = "agent";
  AG.draft = null;
  Object.assign(AG, {
    open: { machine_id: s.machine_id, agent: s.agent, session: s.id },
    page: transcripts.get(transcriptKey({ machine_id: s.machine_id, agent: s.agent, session: s.id }))?.page
      ?? { session: s, plan: [], files: [], entries: [], earlier: 0, last_message: "", error: null, queued: 0, queued_messages: [], turns: 0 },
    why, back: from, note: "", readError: "", link: LINKED, sig: "", rest: "", changed: Date.now(), answering: null,
    att: [], attError: "", noImages: null, opened: new Set(), steps: new Map(), all: new Set(),
    queue: steerQueues.get(transcriptKey({ machine_id: s.machine_id, agent: s.agent, session: s.id })) ?? [],
    held: 0, stopping: null, modelOpen: false, modes: null,
    older: { stop: "", loading: false, gaveUp: false },
  });
  if (AG.open) firstRead = { open: AG.open, at: performance.now() };
  drawAgentFrame(inPlace);
  watchRead(AG.open!);
  pollAgent(true);
}

/// A session opened that still has no read after this long leaves a line in the log.
const READ_WATCH_MS = 30_000;

let reading: { open: OpenAgent; at: number; pending: Set<string> } | null = null;

function traceSession(stage: string, o: OpenAgent, details: Record<string, unknown> = {}) {
  logInfo(`session view: ${JSON.stringify({ stage, session: o.session, machine: o.machine_id, view: S.view, body: !!$("#agbody"), ...visibilitySnapshot(), ...details })}`);
}

/// Logs it when the session `o` opened still shows nothing read after
/// `READ_WATCH_MS` with no error said: the view is stuck, and why (#427).
function watchRead(o: OpenAgent) {
  setTimeout(() => {
    if (AG.open !== o || !isOpen(o) || AG.sig || AG.readError) return;
    logError(`session view: nothing read for session ${o.session} after ${READ_WATCH_MS / 1000} s ${JSON.stringify({ reading: AG.inflight === o, pending: reading?.open === o ? [...reading.pending] : [], view: S.view, body: !!$("#agbody"), ...visibilitySnapshot() })}`);
  }, READ_WATCH_MS);
}

// The open session moved and its new machine lists it: the page follows it there.
sessionsRead.add(() => {
  if (S.view !== "agent" || !AG.open) return;
  const there = movedTo(AG.open, MV.records, sessions);
  if (there) openAgent(there, { back: AG.back });
});

/// The last log drawn and the session it is of, to go back to.
let left: { key: string; body: ChatBody } | null = null;
const sessionKey = (s: AgentRow) => `${s.machine_id}\n${s.id}`;

export const isOpen = (o: OpenAgent | null) => S.view === "agent" && AG.open && o === AG.open;

const transcripts = new TranscriptCache<AgentPage>();

interface FeedUpdate extends TranscriptUpdate<AgentPage> {
  modes: typeof AG.modes;
  tasks: typeof AG.tasks;
}

const feed = new AgentFeed<OpenAgent, FeedUpdate>({
  id: () => crypto.randomUUID(),
  now: () => Date.now(),
  open: (o, id, receive) => {
    const onEvent = new Channel<FeedUpdate>();
    onEvent.onmessage = receive;
    return invoke<void>("agent_follow", { ...where(o), id, last: 200, after: transcripts.get(transcriptKey(o))?.cursor ?? null, onEvent });
  },
  close: id => { void invoke("agent_unfollow", { id }).catch(e => console.warn("session detach:", e)); },
  receive: (o, update) => {
    if (!isOpen(o) || !agentOnScreen()) { feed.stop(); return; }
    const page = transcripts.receive(transcriptKey(o), update);

    if (!page) {
      feed.stop();
      void pollAgent(true);
      return;
    }

    AG.modes = update.modes;
    AG.tasks = update.tasks;
    // Normal updates only compare and redraw their changed suffix.
    applyAgentPage(o, update.kind === "delta" ? update.page : page, ++AG.polls, update.kind === "snapshot");
  },
  disconnected: (o, error) => {
    if (!isOpen(o) || !agentOnScreen()) return;
    console.warn("session stream disconnected:", error);
    const failed = readFailed(AG.link, { kind: "lost" }, AG.page?.session.machine_name || "the machine");
    AG.link = failed.link;
    AG.readError = AG.note = failed.note;
    drawAgentBody();
    void pollAgent();
  },
});

export function stopAgentFollow() {
  clearTimeout(AG.timer);
  feed.stop();
}

changeListeners.push(() => { if (S.view !== "agent") stopAgentFollow(); });

function applyAgentPage(o: OpenAgent, page: AgentPage, read: number, replace = false) {
  if (AG.page) page.session = nativeUpdate(AG.page.session, page.session);

  if (isNativeSession(page.session.id)) {
    const row = sessions.find(s => s.machine_id === page.session.machine_id && s.id === page.session.id);
    if (row) keepNativeSession({ ...row, ...page.session });
  }

  const sig = JSON.stringify(page);
  const moved = sig !== AG.sig;
  if (moved) { AG.sig = sig; AG.changed = Date.now(); }
  if (page.session.question?.id !== AG.answering) AG.answering = null;
  // A session opened with no folder (from a notice) draws again once a read gives it.
  const placed = !AG.page?.session.cwd && !!page.session.cwd;
  // The session closed or was taken over: the composer replaces the banner.
  const field = !!AG.page && followsOnly(AG.page.session) !== followsOnly(page.session);
  const news = hasNews(AG.page, page);
  // The older entries held stay while the newest are read again.
  AG.page = replace ? page : joinRuns(AG.page, page);
  drawModelPill();
  drawAgentHeading();
  // The frame around the log is drawn again; the log itself stays.
  if (placed) drawAgentFrame(false, $<ChatBody>("#agbody"));
  else if (field && $("#agconv")) drawAgentFrame(true, $<ChatBody>("#agbody"));
  // Back after a lost link, or something new came: earlier errors no longer hold.
  AG.note = noteAfterRead({ note: AG.note, failed: AG.readError, link: AG.link, news });
  AG.readError = "";
  AG.link = LINKED;
  AG.stopping = stopPending(AG.stopping, page.session.id, AGENT_ACTIVE.has(page.session.status), page.turns);
  // The node holds the steers: show those another app sent, drop those it took.
  syncSteers(AG.queue, page, read);
  // Nothing the log shows changed: no draw.
  const rest = JSON.stringify([AG.modes, AG.tasks, AG.note, AG.answering, AG.log != null, stoppingNow(), AG.queue.map(q => q.state), placed]);
  if (moved || rest !== AG.rest) { AG.rest = rest; drawAgentBody(undefined, true); }
  finishFirstRead(o, "ok");
  void readUsage(o, page);
  // The oldest turn shown has its prompt: read back until it does.
  if (lacksPrompt(AG.page) && !AG.older.gaveUp) void loadOlder("prompt");
}

/// Whether the session view can be seen: the window shows, and in the
/// command center the pane is mounted.
export const agentOnScreen = paneShown;

/// How long until an older node is asked again. Quiet sessions keep following.
export function agentDelay() {
  const s = AG.page?.session;
  if (!s || s.status === "ended") return AGENT_SLOW_MS;
  const still = Date.now() - AG.changed;
  // A question nobody answers for a while is asked about slowly; an answer
  // from here or a notice reads it at once.
  if (s.status === "asking" && still >= AGENT_SLOW_AFTER) return AGENT_SLOW_MS;
  if (thinking && AGENT_ACTIVE.has(s.status)) return AGENT_THINK_POLL_MS;
  // Once stopped, the turn ending shows as soon as it does.
  if (!s.external && AGENT_ACTIVE.has(s.status)) return stoppingNow() ? AGENT_STOP_POLL_MS : AGENT_POLL_MS;
  return still >= AGENT_SLOW_AFTER ? AGENT_SLOW_MS : AGENT_POLL_MS;
}

/// Asks for the open session, then again after `agentDelay()` while it
/// stays on screen. Opening reads once even if the webview's
/// visibility has not caught up. Never two calls at once for one session.
export async function pollAgent(readNow = false) {
  clearTimeout(AG.timer);
  const o = AG.open;
  if (!o) { feed.stop(); return; }
  const trace = readNow || !AG.sig;
  const skip = !isOpen(o) ? "different-view" : AG.inflight === o ? "in-flight" : !readNow && !agentOnScreen() ? "hidden" : null;

  if (skip) {
    if (skip !== "in-flight") feed.stop();
    if (trace) traceSession("read-skipped", o, { reason: skip, readNow });
    return;
  }

  if (agentOnScreen()) feed.follow(o);
  else feed.stop();
  if (feed.live(o)) return;
  // Give the initial snapshot a chance to arrive before asking an older node for a fallback.
  if (feed.waiting(o)) { AG.timer = setTimeout(pollAgent, 3000); return; }
  const version = feed.version(o);
  AG.inflight = o;
  const read = ++AG.polls;
  const current = { open: o, at: Date.now(), pending: new Set(["view", "modes", "tasks", "processes"]) };
  reading = current;
  if (trace) traceSession("read-started", o, { readNow });
  const tracked = <T>(name: string, call: Promise<T>) => call.finally(() => {
    current.pending.delete(name);
    if (trace) traceSession("read-settled", o, { part: name, ms: Date.now() - current.at, pending: [...current.pending] });
  });

  try {
    const [page, modes, tasks] = await Promise.all([tracked("view", invoke<AgentPage>("agent_view", { ...where(o), last: newestRead(AG.page, AGENT_LAST) }, AGENT_READ_WAIT_MS)), tracked("modes", readModes(o)), tracked("tasks", readTasks(o)), tracked("processes", readRunning())]);

    if (isOpen(o) && feed.version(o) === version) {
      AG.modes = modes;
      AG.tasks = tasks;
      // A push received while this fallback read ran is newer than its reply.
      applyAgentPage(o, page, read);
      if (trace) traceSession("read-applied", o, { status: page.session.status, entries: page.entries.length, ms: Date.now() - current.at });
    }
  } catch (e) {
    if (trace) traceSession("read-failed", o, { pending: [...current.pending], ms: Date.now() - current.at });

    if (isOpen(o) && feed.version(o) === version) {
      // A dropped link is read again with backoff; a session its machine
      // came back without has ended, and no turn runs on.
      const failed = readFailed(AG.link, e, AG.page?.session.machine_name || "the machine");
      AG.link = failed.link;
      AG.readError = AG.note = failed.note;
      if (failed.link.ended && AG.page) AG.page = endedPage(AG.page);
      drawAgentBody();
      finishFirstRead(o, "error");
    }
  } finally {
    if (AG.inflight === o) AG.inflight = null;
    if (reading === current) reading = null;
  }

  // Another session opened while this one was asked: follow that one.
  if (AG.open !== o) { if (isOpen(AG.open) && !AG.inflight) pollAgent(); return; }

  if (!isOpen(o) || !agentOnScreen()) {
    if (trace) traceSession("polling-stopped", o, { reason: !isOpen(o) ? "different-view" : "hidden" });
    return;
  }

  const delay = AG.link.lost ? retryDelay(AG.link.lost) : agentDelay();
  if (!feed.live(o)) AG.timer = setTimeout(pollAgent, delay);
}

/// Follows the open session again after showing it or sending a message.
export function resumeAgent() {
  AG.changed = Date.now();
  pollAgent();
}

// Put a shown log back before a read or a scroll event can mistake a
// hidden view's reset for the reader scrolling up.
function restoreAgentScroll() {
  const b = $<ChatBody>("#agbody");
  if (!b) return;
  b.scrollTop = returnScroll(b, b.top ?? 0);
  scrolledLog(b);
  syncJump(b);
}

// A pane shown again resumes its subscription.
window.addEventListener("agent-pane-shown", () => {
  if (S.view === "agent") resumeAgent();
});

onShownChange(shown => {
  if (AG.open && S.view === "agent") traceSession("visibility-changed", AG.open, { shown });

  if (!shown) { clearTimeout(AG.timer); feed.stop(); }
  else if (S.view === "agent") {
    restoreAgentScroll();
    pollAgent();
  }
}, true);
listen("agent-title-changed", () => {
  if (S.view === "agent") void pollAgent(true);
  void loadSessions().then(() => { if (S.view === "agents") drawAgents(false); }).catch(e => console.warn("session titles:", e));
});

// A notice about a session that asks may mean it was answered elsewhere.
for (const name of ["sessions-changed", "work-resolved"]) {
  listen(name, () => { if (AG.page?.session.status === "asking" && S.view === "agent") pollAgent(); });
}

/// Whether the server turned images down for the open session.
export const noAgentImages = () => AG.noImages != null && AG.noImages === AG.open?.session;

/// Whether the open session's turn runs, so Enter steers and ⌘. stops.
export const turnRuns = () => {
  const s = AG.page?.session;
  return !!s && !s.external && !s.question && AGENT_ACTIVE.has(s.status);
};

/// Sends what the field holds: while the turn runs it steers it after its current tool call.
export function submitAgentMessage(input: HTMLTextAreaElement) {
  if (input.readOnly) return;
  if (turnRuns()) steerMessage(input);
  else void sendAgentMessage();
}

/// Whether the user stopped the open session's turn and it has not ended.
export const stoppingNow = () => {
  const s = AG.page?.session;
  return !!s && AG.stopping?.id === s.id && AGENT_ACTIVE.has(s.status);
};

/// The session's keys on one small line.
export function agentHints() {
  const s = AG.page?.session, q = s?.question;
  const slim = (items: Parameters<typeof hints>[0]) => {
    const open = $("#agshortcuts")?.hasAttribute("open") ? " open" : "";
    const sections = SHORTCUTS.filter(section => ["Agent session", "Side pane", "Terminals"].includes(section.title));
    const rows = sections.map(section => `<section><b>${esc(section.title)}</b>${section.rows.map(row => `<span><kbd>${esc(row.keys)}</kbd>${esc(row.does)}</span>`).join("")}</section>`).join("");
    const project = AG.draft ?? (s && windowHooks?.sessionProject?.(s)?.name) ?? null;
    return `<div class="agkeys">${hints(items).replace('class="dhints"', 'class="dhints slim"')}${stashIndicator(project)}<details id="agshortcuts"${open}><summary>Show more</summary><div class="agkeys-list">${rows}${stashActions()}</div></details></div>`;
  };
  if (AG.draft) return slim([["⌘T", "terminal"], ["⌘2", "files"], ["esc", "back"]]);
  const mode: Parameters<typeof hints>[0] = hasModes() ? [["⇧Tab", "mode"]] : [];
  if (q && AG.modes?.plan != null) return slim([[`⌘${ICON.ret}`, "approve"], ["⌘D", "keep planning"], ...mode, ["esc", "back"]]);
  if (q) return slim([[`⌘${ICON.ret}`, "allow"], ["⌘D", "deny"], ["esc", "back"]]);
  if (s && isNativeSession(s.id)) return slim([["esc", "parent session"]]);
  if (s && followsOnly(s)) return slim([["esc", "back"]]);
  if (s?.status === "ended") return slim([[ICON.ret, "continue"], ["esc", "back"]]);

  if (turnRuns()) {
    const keys: Parameters<typeof hints>[0] = [];
    if (stoppingNow()) return slim(keys);
    return slim([...keys, ...mode, ["⌘.", "stop"], ["esc", "back"]]);
  }

  return slim([...mode, ["⌘U", "thoughts"], ["⌘Y", "commands"], ["esc", "back"]]);
}

/// Puts the session's key hints in place of those on screen.
export function redrawAgentHints() {
  const input = $<HTMLTextAreaElement>("#agmsg"), s = AG.page?.session;
  if (input && s) input.placeholder = AG.att.length ? "Add a message (optional)" : s.status === "ended" ? `Message ${agentKind(s.agent).name} to continue` : `Message ${agentKind(s.agent).name}`;
  const hint = $("#agconv > .agkeys");
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
export const SWITCHES = new Set(["claude", "codex", "opencode", "flsh", "hermes"]);
/// The model and thinking switched to in each session from here, by session.
export const sessionPicks = new SessionPicks();

/// The session the mentions waiting above the field belong to.
let paneSessionKey = "";

/// The header's usage line, the session it is of, and the turns it counts.
let usage = { of: "", turns: -1, text: "" };
const usageKey = (s: AgentRow) => `${s.machine_id}\n${s.agent}\n${s.id}`;

/// Reads the open session's usage once per finished turn into the header.
async function readUsage(o: OpenAgent, page: AgentPage) {
  const s = page.session, of = usageKey(s);
  if (s.external || (usage.of === of && usage.turns === page.turns)) return;
  usage = { of, turns: page.turns, text: usage.of === of ? usage.text : "" };

  try {
    const u = await invoke<AgentUsage | null>("agent_usage", where(o));
    if (usage.of !== of || usage.turns !== page.turns || !u) return;
    usage.text = usageLine(u);
    const line = $("#aguse");
    if (line && isOpen(o)) line.textContent = usage.text;
  } catch (e) {
    console.warn("agent usage not read", e);
    if (usage.of === of) usage.turns = -1;
  }
}

/// The folder on this Mac the head's Open in button opens, for ⌘O.
let editorFolder: string | null = null;
let editorHead: { target: HTMLElement; key: string } | null = null;

/// ⌘O: the folder in the first editor, when this Mac has it.
export function openEditorKey() {
  if (editorFolder) void openInEditor(editorFolder);
}

/// Open in an editor, in the head, when this Mac has the folder `cwd` of
/// `machine`; `current` says the head still belongs to it.
export async function drawEditors(machine: string, cwd: string, current: () => boolean) {
  const target = $("#agedit"), key = JSON.stringify([machine, cwd]);
  if (!target || (editorHead?.target === target && editorHead.key === key)) return;
  const head = editorHead = { target, key };
  editorFolder = null;
  target.replaceChildren();
  const folder = await invoke<string | null>("session_folder_here", { machineId: machine, cwd })
    .catch(e => { console.warn("session folder not read", e); return null; });
  const draw = () => {
    if (!current() || editorHead !== head || $("#agedit") !== target) return;
    editorFolder = folder;
    target.replaceChildren(...editorTools(folder, draw));
  };
  draw();
}

// Titles and checkout details can arrive after the frame first draws.
function drawAgentHeading() {
  const s = AG.page?.session;
  if (!s) return;
  const title = $("#agtitle");
  if (!title) return;
  if (!title.querySelector("input")) {
    title.textContent = s.title || agentKind(s.agent).name;
    title.title = `${title.textContent}\nDouble-click to rename (Enter)`;
  }
  const project = windowHooks?.sessionProject?.(s), place = placeOf(s);
  const folder = $("#agproj"), branch = $("#agbranch");
  if (folder) folder.textContent = project?.label || (place ? placeRepo(place) : shortCwd(s.cwd)) || agentKind(s.agent).name;
  if (branch) branch.textContent = place?.linked ? ` · ${placeName(place)}` : "";
  const here = $("#aghere");
  // Keep the machine menu when checkout details change.
  if (here && !here.querySelector(".mvb")) here.textContent = s.machine_name;
  void drawEditors(s.machine_id, s.cwd, () => !!AG.page && usageKey(AG.page.session) === usageKey(s));
}
onPlaces(drawAgentHeading);


/// Closes the open session, preserving its conversation for later.
export async function closeOpenSession() {
  const s = AG.page?.session, o = AG.open;
  if (AG.draft || !s || !o || shownStatus(s) === "closing" || !sessionOffers({ ...s, status: shownStatus(s) }).close) return;
  const closing = closeSession(s);
  // Leave before the machine acknowledges the close.
  agentBack(true);

  try {
    await closing;
  } catch (e) {
    if (isOpen(o) && agentOnScreen()) {
      AG.note = readError(e);
      drawAgentBody();
    } else {
      const banner = failBanner(`Could not close session: ${readError(e)}`, () => banner.remove());
      Object.assign(banner.style, { position: "fixed", inset: "auto 16px 16px", zIndex: "1000", background: "var(--ink)" });
      document.body.append(banner);
    }
  }
}

/// A session started outside Beep that only shows here: still running there,
/// or of an agent Beep cannot take over. A closed one continues from the field.
function followsOnly(s: AgentRow) {
  return isNativeSession(s.id) || (s.external && !(s.status === "ended" && canTakeOver(s.agent)));
}

/// The parts that stay while the session updates: the head, the body and the field.
/// With `inPlace` only the head and the conversation are replaced, so the pane
/// beside them, its terminals running, is not drawn again. `kept`, the log
/// already drawn, goes back in as it is, where it was scrolled.
export function drawAgentFrame(inPlace = false, kept: ChatBody | null = null) {
  const s = AG.page!.session, k = agentKind(s.agent);
  const field = isNativeSession(s.id)
    ? `<div class="dinput small" id="agfield"><textarea id="agnativemsg" rows="1" aria-label="Messages are sent from the parent conversation" disabled placeholder="Open the parent conversation to send a message"></textarea><span id="agmodel"></span><button class="dbtn" id="agnativeparent">Open parent</button></div>`
    : followsOnly(s)
    ? `<div class="dmuted dblock dbanner">${canTakeOver(s.agent) ? "Running outside Beep. Beep follows it here. Close it there to continue here." : "Started outside Beep. Beep follows it here."}</div>`
    : `<div id="agmovebox"></div><div id="agplan"></div><div id="agtasks"></div><div class="dinput small" id="agfield"><textarea id="agmsg" rows="1" autocomplete="off" spellcheck="false" placeholder="Message ${esc(k.name)}"></textarea><span id="agmode"></span><span id="agmodel"></span><button class="dbtn" id="agstop" aria-label="Stop the turn" title="Stop the turn (⌘.)">${stopFace()}</button><button class="dbtn" id="agsend" aria-label="Send">${ICON.send}</button></div><div id="agpickm"></div>`;
  // The conversation fills the window; the media pane sits beside it or under it.
  const project = windowHooks?.sessionProject?.(s);
  const head = `<div class="dhead">${agentAvatar(s.agent)}<span class="tt"><b id="agtitle" title="${esc(s.title || k.name)}">${esc(s.title || k.name)}</b><small class="aglocation" title="${esc(s.cwd)}"><span class="agfolder">${project ? `<button class="agproj" id="agproj">${esc(project.label)}</button>` : `<span id="agproj">${esc(shortCwd(s.cwd) || k.name)}</span>`}<span id="agbranch"></span></span><span>@</span><span id="aghere">${esc(s.machine_name)}</span></small></span><small id="aguse" class="aguse" title="Context used / capacity · session tokens · session spend / remaining credits">${esc(usage.of === usageKey(s) ? usage.text : "")}</small><span id="agstate">${stateTag(s.status)}</span><span id="agedit" class="agedit"></span></div>`;
  const conv = `<div class="agconv" id="agconv"><div class="dbody" id="agbody"></div>${field}${agentHints()}</div>`;
  // Out of the page a box reads 0: it keeps where the reader left it.
  const keptTop = kept ? (kept.isConnected ? kept.scrollTop : kept.top ?? 0) : 0;
  if (inPlace) {
    bar.querySelector(":scope > .dhead")!.outerHTML = head;
    $$("#agconv").outerHTML = conv;
  } else {
    bar.innerHTML = `${head}<div class="agshell" id="agshell">${paneFrame(conv)}</div>`;
  }
  if (paneSessionKey !== `${s.machine_id}\n${s.id}`) { paneSessionKey = `${s.machine_id}\n${s.id}`; resetMentions(); }
  if (project) $("#agproj")?.addEventListener("click", project.open);
  const open = AG.open;
  bindTitleEdit($$("#agtitle"), {
    value: () => AG.page?.session.title || agentKind(s.agent).name,
    save: async title => {
      await invoke("agent_title_save", { machineId: s.machine_id, agent: s.agent, session: s.id, title });
      if (AG.open !== open) return;
      if (AG.page) AG.page.session.title = title || null;
      await pollAgent(true);
      void loadSessions().catch(e => console.warn("session titles:", e));
    },
  });
  drawAgentHeading();
  MN.redraw = drawAgentAttached;
  MN.error = agentAttachError;
  const place = { ...s, project: project?.name };
  if (inPlace) rebindPane(place); else startPane(place);
  void refreshMoves().then(drawMove);
  const msg = $<HTMLTextAreaElement>("#agmsg");

  if (msg) {
    const picker = filePicker(agentDrop);
    msg.before(picker.button, picker.input);
    composeField(msg, { images: agentAttach, grow: 156, draft: JSON.stringify([s.machine_id, s.agent, s.id]) });
    bindMentionMenu(msg);
    bindSessionStash(msg);
  }

  drawAgentAttached();
  drawModelPill();
  const b = kept ?? $$<ChatBody>("#agbody");
  left = { key: sessionKey(s), body: b };

  if (kept) {
    $$("#agbody").replaceWith(kept);
    kept.scrollTop = returnScroll(kept, keptTop);
  } else {
    b.stuck = true;
    // Once a frame, whatever the scroll events: a move up lets go of the end
    // at once, and a wheel turning holds off pins to the end (`pinEnd`).
    let frame = 0, top = b.scrollTop;
    b.onscroll = () => {
      frame ||= requestAnimationFrame(() => {
        frame = 0;
        if (!b.isConnected || !agentOnScreen()) return;
        b.stuck = followsAfterScroll(b.stuck !== false, top, b);
        scrolledLog(b);
        top = b.top = b.scrollTop;
        syncJump(b);
        if (nearTop(b)) void loadOlder("near");
      });
    };
    b.addEventListener("wheel", () => { b.wheel = performance.now(); }, { passive: true });
  }

  b.oncontextmenu = e => fileLinkMenu(e, { machineId: s.machine_id, root: s.cwd }, openLinked);
  drawAgentBody();
  $("#agmsg")?.focus();
  bar.onclick = agentClick;
}

/// Opens the agent a card names. A child started after the list was read is not in it yet, so a miss reads it again.
async function kidSession(id: string, key?: string) {
  const row = childRow(sessions, id, key);
  if (row) return row;
  try { await loadSessions(); } catch { return null; }
  return childRow(sessions, id, key);
}

async function openKid(id: string, key?: string) {
  const row = await kidSession(id, key);
  if (row) openAgent(row, { back: AG.back });
}

/// Stop on a running child's row: stops its turn, as Stop in its own log does.
async function stopKid(x: HTMLElement) {
  const kid = x.closest<HTMLElement>("[data-kid]");
  const row = kid && await kidSession(kid.dataset.kid!, kid.dataset.key);
  if (!row) return;

  try {
    await invoke("agent_steer", { machineId: row.machine_id, agent: row.agent, session: row.id, steer: "cancel" });
  } catch (e) {
    AG.note = readError(e);
    drawAgentBody();
  }
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
  const closing = target.closest<HTMLElement>(".agpane [data-subclose]");
  if (closing) return closeSub(Number(closing.dataset.subclose));
  const stopping = target.closest<HTMLElement>(":is(#agbody, .agpane) [data-kidstop]");
  if (stopping) return void stopKid(stopping);
  const prompt = target.closest<HTMLElement>("#agbody [data-prompt]");

  if (prompt) {
    const at = Number(prompt.dataset.prompt);
    AG.steps.set(at, prompt.getAttribute("aria-expanded") !== "true");
    return drawAgentBody(clickedRow(prompt));
  }

  const summary = target.closest<HTMLElement>("[data-subsummary]");
  if (summary) { const at = Number(summary.dataset.subsummary); AG.steps.set(at, !AG.steps.get(at)); drawAgentBody(); return; }
  const sub = target.closest<HTMLElement>(":is(#agbody, .agpane) [data-native]");

  if (sub && AG.page && AG.log) {
    const entry = AG.page.entries[Number(sub.dataset.native) - AG.page.earlier];
    if (!entry) return;
    const { title, status, output } = AG.log.parseTool(AG.log.imageLines(entry.text).text);
    const child = AG.log.subagent(output);
    const row = child?.session && nativeSession(AG.page.session, child.session, title.split("\n")[0], status);
    if (row) { keepNativeSession(row, AG.page.session); openAgent(row, { back: AG.back }); }
    return;
  }

  // An agent another agent started opens as its own session.
  const kid = target.closest<HTMLElement>(":is(#agbody, .agpane) [data-kid]");
  if (kid) { void openKid(kid.dataset.kid!, kid.dataset.key); return; }
  const up = target.closest("#agup, #agup2, #agnativeparent");
  if (up) { const row = AG.open && AG.page && parentSession(AG.page.session); if (row) openAgent(row, { back: AG.back }); return; }
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

  // The files a turn made fold and open in place, like a run of calls.
  const madeHead = target.closest<HTMLElement>("#agbody [data-madeat]");

  if (madeHead && AG.open) {
    const opened = turnsOf(AG.open.session).made, at = Number(madeHead.dataset.madeat);
    const open = !opened.delete(at);
    if (open) opened.add(at);
    madeHead.parentElement?.classList.toggle("open", open);
    madeHead.setAttribute("aria-expanded", String(open));
    return;
  }

  // A file a turn made opens in the Artifacts tab, or else in its own tab.
  const madeFile = target.closest<HTMLElement>("#agbody [data-made]");

  if (madeFile) {
    if (!showArtifact(madeFile.dataset.made!)) openLinked(madeFile.dataset.made!);
    return;
  }

  const copy = target.closest<HTMLElement>("#agbody [data-copy]");
  if (copy) return copyCode(copy);
  if (target.closest("#agjump")) { jumpToLatest(); return; }
  // A thought opens and folds in place, so only it animates.
  const thought = target.closest<HTMLElement>("#agbody [data-thought]");

  if (thought && AG.open) {
    const m = turnsOf(AG.open.session), at = Number(thought.dataset.thought);
    if (toggleThought(thought)) m.thoughts.add(at);
    else m.thoughts.delete(at);
    return;
  }

  // The heading toggles the steps; the bottom chevron returns to it.
  const turn = target.closest<HTMLElement>("#agbody [data-turn]");

  if (turn && AG.open) {
    const opened = turnsOf(AG.open.session).opened, key = Number(turn.dataset.turn);
    const collapse = turn.hasAttribute("data-collapse");
    const heading = `#agbody .hd[data-turn="${key}"]`;
    // Bring the heading into view before the shorter turn is virtualized.
    if (collapse) $(heading)?.scrollIntoView({ block: "nearest" });
    if (!opened.delete(key) && !collapse) opened.add(key);
    drawAgentBody(clickedRow(turn));

    if (collapse) {
      const head = $<HTMLButtonElement>(heading);
      head?.focus({ preventScroll: true });
      head?.scrollIntoView({ block: "nearest" });
    }

    return;
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

  if (tool && !getSelection()!.toString() && !target.closest(".out, .ddiff, .cmd, .call-details")) {
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

  if (target.closest("#agresume")) return resumeAgent();
  if (target.closest("#agstop")) return void interruptAgent();

  if (target.closest("#agsend")) {
    const input = $<HTMLTextAreaElement>("#agmsg");
    if (input) submitAgentMessage(input);
  }
}

/// Replayed prompts can have more text after their image lines.
export const userImages = imageLines;

// Tool rows also open from the keyboard; details stay selectable.
document.addEventListener("keydown", event => {
  const target = event.target;
  if (!(target instanceof HTMLElement) || !target.matches('#agbody .dcoderen.tool[data-at][role="button"]') || !["Enter", " "].includes(event.key)) return;
  event.preventDefault();
  target.click();
});
