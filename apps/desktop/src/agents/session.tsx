// One coding agent session in the bar: opening it, drawing its frame and
// reading it while it shows. Following it is in session-follow.ts, its head
// in session-head.ts and what a click in it does in session-click.ts.
import { SessionField, SessionHead } from "./session-frame.tsx";
import { agentMessage, sessionBody, showSessionFrame } from "./session-frame-state.ts";
import { followsOnly, isOpen, keepLog, keptLogs, readAsked, resumeFollow, sessionAsked, sessionPicks, setNote, setReadNote, stoppingNow, turnRuns, type KeptLog } from "./open-state.ts";
import { bindSessionStash } from "./stash.ts";
import { transcriptKey, transcripts } from "./transcript-cache-state.ts";
import { agentDrop } from "../chat/attach.tsx";
import { fileLinkMenu } from "./filelinks.ts";
import { agentKey, conversationHistory, conversationKey, retitle, sessionKey, type Retitled } from "./conversation-state.ts";
import { stopPending } from "./agentqueue.ts";
import { $, type AgentReturn, bar, S, windowHooks, setView } from "../bar/core.ts";
import { showFailure } from "../ui/failbanner-view.tsx";
import { openLinked, rebindPane, restorePane, startPane } from "../agentpane/media.ts";
import { followsAfterScroll, pinWait, returnScroll } from "./logscroll.ts";
import { bindMentionMenu } from "./mentionmenu.ts";
import { MN, activateMentions, rememberMentionImages, setAttachError } from "./mentions.ts";
import { agentBack } from "./agents-list.ts";
import { type AgentRow, type AgentPage, type OpenAgent, AG, openModes, openTasks, sessionUsage, AGENT_ACTIVE, AGENT_LAST, AGENT_READ_WAIT_MS, canTakeOver, openOf, where } from "./agents-state.ts";
import { agentKind } from "./agent-kind.ts";
import { agentAttachError, drawAgentAttached, removeLastAttachment } from "./attachments-view.tsx";
import { keepNativeSession, sessions, sessionsRead } from "./sessions.ts";
import { closeSession, shownStatus } from "./closing.ts";
import { sessionOffers } from "./sessionmenu-state.ts";
import { drawAgentBody, syncJump, scrolledLog } from "./logview.tsx";
import { drawModelPill } from "./modelpill.tsx";
import { readModes } from "./modepill.tsx";
import { readTasks } from "./tasks.tsx";
import { disposeQueue, interruptAgent, readOutbox, sendAgentMessage, sendPrompt, steerMessage } from "./messages.ts";
import type { ChatBody } from "../ui/turn.ts";
import { invoke, listen, logError, logInfo, recordTiming } from "../ui/tauri.ts";
import { perfFresh } from "../ui/perf.ts";
import { onShownChange, paneShown, visibilitySnapshot } from "../ui/active.ts";
import { onPhone } from "../ui/platform.ts";
import { readError } from "../agentpane/read-file.ts";
import { loadOlder } from "./older.ts";
import { joinRuns, newestRead } from "./older-state.ts";
import { adoptKeptPage, endPage, mergePage, openPage, setFirstMessage, setPageSession } from "./open-page.ts";
import { hasNews, LINKED, noteAfterRead, readFailed, RETRY_MS, viewError } from "./reconnect-state.ts";
import { drawMove } from "./move.ts";
import { moveStore, refreshMoves } from "./move-store.ts";
import { movedTo } from "./move-state.ts";
import { sameSession } from "./running-state.ts";
import { isNativeSession, nativeUpdate } from "./native-session-state.ts";
import { type AgentStarted, isStarting } from "./starting-state.ts";
import { feed, flushDraw, forgetUnfollowedExtras, keptTranscript, pushedPage } from "./session-follow.ts";
import { drawAgentHeading, headText, redrawAgentHints, agentHints } from "./session-head.tsx";
import { agentClick, openParent, toolKey } from "./session-click.ts";
import { askContinueHere, continueAfterTurn, keepContinuing } from "./continue-here.ts";

/// The session each start kept on this device became, by the start's id: a page that still
/// names the start opens the session.
const became = new Map<string, AgentRow>();

/// Opens session `s` (a session row) and follows it while it is on screen. With `adopt`,
/// the empty view that started it, or the start it was until its machine named it, becomes
/// the session in place: its pane, terminal tabs included, stays as it is.
export function openAgent(s: AgentRow, { why = "", back: from, adopt = false }: { why?: string; back?: AgentReturn; adopt?: boolean } = {}) {
  s = became.get(s.id) ?? s;
  conversationHistory.visit(s);
  traceSession("opening", openOf(s), { status: s.status, adopt });

  // Already open: keep its log, draft and queue.
  if (!why && sameSession(AG.open, s) && AG.page?.session.id === s.id) {
    if (AG.page.session.status === "ended" && s.status !== "ended") {
      clearTimeout(AG.timer);
      feed.drop(AG.open!);
      setPageSession(s);
      AG.sig = "";
      drawAgentHeading();
      drawModelPill();
      redrawAgentHints();
    }
    if (AG.open) firstRead = { open: AG.open, at: performance.now() };
    if (from !== undefined) AG.back = from;
    const shown = sessionBody();

    if (S.view === "agent" && shown) {
      restorePane();
      restoreAgentScroll(followLatest(shown));
      return void pollAgent(true);
    }

    if (from === undefined) AG.back = S.view === "agents" ? "agents" : "hide";
    setView("agent");
    Object.assign(AG, { sig: "", rest: "" });
    drawAgentFrame(false, followLatest(keptLogs.get(sessionKey(s))?.body ?? null));
    watchRead(AG.open!);
    return void pollAgent(true);
  }

  if (from === undefined) from = S.view === "agents" ? "agents" : "hide";
  // `s` is what the open start became: Auto's reason for its machine stays.
  const wasStart = !!AG.open && became.get(AG.open.session)?.id === s.id;
  if (wasStart) why ||= AG.why;
  const inPlace = adopt && (!!AG.draft || wasStart) && S.view === "agent" && !!$("#agshell");
  rememberMentionImages();
  leaveLog();
  setView("agent");
  AG.draft = null;
  const open: OpenAgent = openOf(s);
  const log = inPlace ? null : keptLogs.get(sessionKey(s)) ?? null;
  const held = keptPage(log, transcripts.get(transcriptKey(open))?.page ?? null);
  openPage(s, held);
  Object.assign(AG, {
    open,
    why, back: from, note: "", readError: "", link: LINKED, sig: "", rest: "", changed: Date.now(), answering: null,
    opened: new Set(), steps: new Map(), all: new Set(),
    outbox: [], outboxRevision: 0, stopping: null, modelOpen: false,
    older: { stop: "", reading: null },
  });
  setAttachError("");
  firstRead = { open, at: performance.now() };
  drawAgentFrame(inPlace, followLatest(held && log && held.earlier <= log.page.earlier ? log.body : null));
  watchRead(open);
  // A start has no outbox and no kept page.
  if (isStarting(s.id)) return void pollAgent(true);
  void readOutbox(open);
  if (held) return void pollAgent(true);
  // Not held in this window: the page kept on disk draws at once, and the follow goes on from it.
  readingKept = open;
  void keptTranscript(open).then(page => {
    if (readingKept === open) readingKept = null;
    if (AG.open !== open) return;
    // The session row it opened with is newer than the kept one, and the steers it held went long ago.
    if (page && !AG.sig && AG.page) {
      adoptKeptPage(page);
      if (isOpen(open)) {
        drawAgentHeading();
        drawAgentBody();
        keptFresh = { open, done: perfFresh("session") };
      }
    }

    void pollAgent(true);
  });
}

readAsked.subscribe(({ due }) => {
  if (!due) return;
  readAsked.setState({ due: null });
  void pollAgent(due.now);
});
sessionAsked.subscribe(({ wanted }) => {
  if (!wanted) return;
  sessionAsked.setState({ wanted: null });
  openAgent(wanted.row, { back: wanted.back });
});
pushedPage.follow(({ open, page, read, replace }) => applyAgentPage(open, page, read, replace, true));

/// The parts that stay while the session updates: the head, the body and the field.
/// With `inPlace` only the head and the conversation are replaced, so the pane
/// beside them, its terminals running, is not drawn again. `kept`, the log
/// already drawn, goes back in as it is, where it was scrolled.
export function drawAgentFrame(inPlace = false, kept: ChatBody | null = null) {
  const page = AG.page!, s = page.session, k = agentKind(s.agent);
  disposeQueue();
  const project = windowHooks?.sessionProject?.(s);
  // Out of the page a box reads 0: it keeps where the reader left it.
  const keptTop = kept ? (kept.isConnected ? kept.scrollTop : kept.top ?? 0) : 0;
  const b = kept ?? document.createElement("div") as ChatBody;
  if (!kept) { b.className = "dbody"; b.id = "agbody"; }
  const starting = isStarting(s.id);
  const mode = starting ? "starting" : isNativeSession(s.id) ? "native" : followsOnly(s) ? "readonly" : "writable";
  const takeover = canTakeOver(s.agent) && !isNativeSession(s.id);
  const banner = starting ? s.doing ?? "It did not start." : takeover ? "Running outside Beep. Beep follows it here." : "Started outside Beep. Beep follows it here.";
  const continueHere = takeover ? { target: agentKey(s), start: () => askContinueHere(s) } : null;
  keepContinuing(s);
  // The saved title shows at once; `agent-title-changed` brings it to the lists.
  const saveTitle = async (title: string) => {
    await invoke("agent_title_save", { ...where(openOf(s)), title });
    await retitleOpen({ ...openOf(s), title: title || null });
  };
  const head = <SessionHead agent={s.agent} {...headText(s)} openProject={project?.open ?? null} windowProps={project ? windowHooks?.projectWindowProps?.(project.name) ?? null : null} cwd={s.cwd} machine={s.machine_name} machineId={s.machine_id} session={s.id} usage={page.usage ? sessionUsage(page.usage) : null} moveKey={agentKey(s)} saveTitle={starting ? null : saveTitle} />;
  const field = <SessionField mode={mode} name={k.name} banner={banner} moveKey={agentKey(s)} machine={s.machine_id} session={s.id} continueHere={continueHere} attach={agentDrop} draft={conversationKey(s)} removeLast={removeLastAttachment} send={submitAgentMessage} stop={() => void interruptAgent()} openParent={openParent} retry={starting && s.status !== "starting" ? () => void retryStart() : null} cancel={() => void closeOpenSession()} />;
  showSessionFrame({ head, field, body: b, hints: agentHints() }, inPlace, disposeQueue);
  activateMentions(s);
  drawAgentHeading();
  MN.redraw = drawAgentAttached;
  MN.error = agentAttachError;
  const place = { ...s, project: project?.name, sendReview: sendPrompt };
  if (inPlace) rebindPane(place); else startPane(place);
  void refreshMoves().then(drawMove);
  const msg = agentMessage.current;

  if (msg) {
    bindMentionMenu(msg);
    bindSessionStash(msg);
  }

  drawAgentAttached();
  drawModelPill();
  keepLog(sessionKey(s), b, page);

  // A kept log at its end goes there as `drawAgentBody` fills it, with no layout read here.
  if (kept) {
    if (kept.stuck === false) kept.scrollTop = keptTop;
  } else {
    b.stuck = true;
    // Release the end immediately; redraw visible turns once per frame.
    // Wheel and touch gestures hold off pins to the end (`pinEnd`).
    let frame = 0, top = b.scrollTop, height = b.clientHeight, total = b.scrollHeight;
    b.onscroll = () => {
      if (!b.isConnected || !paneShown()) return;
      const layoutChanged = height !== b.clientHeight || total !== b.scrollHeight;
      const readerScrolling = b.touching || pinWait(performance.now(), b.wheel) > 0;
      b.stuck = followsAfterScroll(b.stuck !== false, top, b, layoutChanged && !readerScrolling);
      height = b.clientHeight;
      total = b.scrollHeight;
      top = b.top = b.scrollTop;
      frame ||= requestAnimationFrame(() => {
        frame = 0;
        if (!b.isConnected || !paneShown()) return;
        scrolledLog(b);
        top = b.top = b.scrollTop;
        height = b.clientHeight;
        total = b.scrollHeight;
        syncJump(b);
      });
    };
    b.addEventListener("touchstart", () => { b.touching = true; b.wheel = performance.now(); }, { passive: true });
    b.addEventListener("touchmove", () => {
      b.wheel = performance.now();
      b.stuck = false;
    }, { passive: true });
    const touchEnd = () => { b.touching = false; b.wheel = performance.now(); };
    b.addEventListener("touchend", touchEnd, { passive: true });
    b.addEventListener("touchcancel", touchEnd, { passive: true });
    b.addEventListener("wheel", () => { b.wheel = performance.now(); }, { passive: true });
    b.addEventListener("keydown", toolKey);
  }

  b.oncontextmenu = e => fileLinkMenu(e, { machineId: s.machine_id, root: s.cwd }, s.id, openLinked);
  drawAgentBody();
  // On a touch layout a focused composer raises the keyboard and lays the page out for it.
  if (!document.body.classList.contains("mobile")) agentMessage.current?.focus();
  bar.onclick = agentClick;
}

/// Asks for the open session, and opens its follow again after `RETRY_MS`
/// while it stays on screen without one. Opening reads once even if the webview's
/// visibility has not caught up. Never two calls at once for one session.
export async function pollAgent(readNow = false) {
  clearTimeout(AG.timer);
  const o = AG.open;
  if (!o) { feed.leave(); return; }
  const trace = readNow || !AG.sig;
  const skip = !isOpen(o) ? "different-view" : AG.inflight === o || readingKept === o ? "in-flight" : !readNow && !paneShown() ? "hidden" : null;

  if (skip) {
    if (skip !== "in-flight") feed.leave();
    if (trace) traceSession("read-skipped", o, { reason: skip, readNow });
    return;
  }

  // A start has no stream: this device answers its reads.
  if (paneShown() && !isStarting(o.session)) feed.follow(o);
  else feed.leave();

  // Followed all along: what came while it was away is drawn once from its model.
  if (feed.live(o)) {
    flushDraw();
    if (!AG.sig) shownFromModel(o);
    // Shown again with its log already drawn: it is ready now, not at the next change.
    else finishFirstRead(o, "ok");
    void loadOlder();
    return;
  }

  // The follow answers with the page; a read is only for when it fails.
  if (feed.waiting(o)) return;
  const version = feed.version(o);
  AG.inflight = o;
  const read = ++AG.polls;
  const current = { open: o, at: Date.now(), pending: new Set(["view", "modes", "tasks"]) };
  reading = current;
  if (trace) traceSession("read-started", o, { readNow });
  const tracked = <T,>(name: string, call: Promise<T>) => call.finally(() => {
    current.pending.delete(name);
    if (trace) traceSession("read-settled", o, { part: name, ms: Date.now() - current.at, pending: [...current.pending] });
  });

  const still = () => isOpen(o) && feed.version(o) === version;
  // The page shows as soon as it comes; modes and tasks join it as each arrives.
  void tracked("modes", readModes(o)).then(modes => { if (still()) { transcripts.setModes(transcriptKey(o), modes); drawRest(); } });
  void tracked("tasks", readTasks(o)).then(tasks => { if (still()) { transcripts.setTasks(transcriptKey(o), tasks); drawRest(); } });

  try {
    const page = await tracked("view", invoke<AgentPage>("agent_view", { ...where(o), last: newestRead(AG.page, AGENT_LAST, window.innerHeight) }, AGENT_READ_WAIT_MS));

    if (still()) {
      // A push received while this fallback read ran is newer than its reply.
      flushDraw(true);
      applyAgentPage(o, page, read);
      if (trace) traceSession("read-applied", o, { status: page.session.status, entries: page.entries.length, ms: Date.now() - current.at });
    }
  } catch (e) {
    if (trace) traceSession("read-failed", o, { pending: [...current.pending], ms: Date.now() - current.at });

    if (still() && isStarting(o.session) && viewError(e).kind === "gone") {
      leaveStart(o.session);
    } else if (still()) {
      // A dropped link is read again with backoff; a session its machine
      // came back without has ended, and no turn runs on.
      const failed = readFailed(AG.link, e, AG.page?.session.machine_name || "the machine");
      AG.link = failed.link;
      setReadNote(failed.note);
      if (failed.link.ended) endPage();
      drawAgentBody();
      finishFirstRead(o, "error");
    }
  } finally {
    if (AG.inflight === o) AG.inflight = null;
    if (reading === current) reading = null;
  }

  // Another session opened while this one was asked: follow that one.
  if (AG.open !== o) { if (isOpen(AG.open) && !AG.inflight) pollAgent(); return; }

  if (!isOpen(o) || !paneShown()) {
    if (trace) traceSession("polling-stopped", o, { reason: !isOpen(o) ? "different-view" : "hidden" });
    return;
  }

  // The follow is opened again every `RETRY_MS`. A start says when it changes (`sessions-changed`), and an ended session does not change.
  const quiet = isStarting(o.session) || AG.page?.session.status === "ended";
  if (!feed.live(o) && (AG.link.lost || !quiet)) AG.timer = setTimeout(pollAgent, RETRY_MS);
}

let firstRead: { open: OpenAgent; at: number } | null = null;
/// The session whose page kept on disk is being read: its follow starts after, from its cursor.
let readingKept: OpenAgent | null = null;
/// Times a session from its page kept on disk to its first fresh read.
let keptFresh: { open: OpenAgent; done: () => void } | null = null;
function finishFirstRead(open: OpenAgent, outcome: "ok" | "error") {
  if (keptFresh?.open === open) { if (outcome === "ok") keptFresh.done(); keptFresh = null; }
  if (!firstRead || firstRead.open !== open) return;
  recordTiming("data", "agent", performance.now() - firstRead.at, outcome);
  firstRead = null;
}

/// A session opened that still has no read after this long leaves a line in the log.
const READ_WATCH_MS = 30_000;

let reading: { open: OpenAgent; at: number; pending: Set<string> } | null = null;

function traceSession(stage: string, o: OpenAgent, details: Record<string, unknown> = {}) {
  logInfo(`session view: ${JSON.stringify({ stage, session: o.session, machine: o.machine_id, view: S.view, body: !!sessionBody(), ...visibilitySnapshot(), ...details })}`);
}

/// Logs it when the session `o` opened still shows nothing read after
/// `READ_WATCH_MS` with no error said: the view is stuck, and why (#427).
function watchRead(o: OpenAgent) {
  setTimeout(() => {
    if (AG.open !== o || !isOpen(o) || AG.sig || AG.readError) return;
    logError(`session view: nothing read for session ${o.session} after ${READ_WATCH_MS / 1000} s ${JSON.stringify({ reading: AG.inflight === o, pending: reading?.open === o ? [...reading.pending] : [], view: S.view, body: !!sessionBody(), ...visibilitySnapshot() })}`);
  }, READ_WATCH_MS);
}

// The open session moved and its new machine lists it: the page follows it there.
sessionsRead.add(() => {
  if (S.view !== "agent" || !AG.open) return;
  const there = movedTo(AG.open, moveStore.getState().records, sessions);
  if (there) openAgent(there, { back: AG.back });
});

/// The open session's kept log takes its page as the reader leaves it.
export function leaveLog() {
  const log = AG.open && keptLogs.get(sessionKey({ machine_id: AG.open.machine_id, id: AG.open.session }));
  if (log && AG.page) log.page = AG.page;
  forgetUnfollowedExtras(AG.open);
}

/// The page a session opens with: the one held in this window, or the kept log's when it
/// reaches further back, with the newest held joined on.
function keptPage(log: KeptLog | null, held: AgentPage | null): AgentPage | null {
  if (!log || (held && held.earlier <= log.page.earlier)) return held;
  return held ? joinRuns(log.page, held) : log.page;
}

/// A `pushed` update always holds something new; a polled one is compared
/// with the last read.
export function applyAgentPage(o: OpenAgent, page: AgentPage, read: number, replace = false, pushed = false) {
  if (AG.page) page = { ...page, session: nativeUpdate(AG.page.session, page.session) };

  if (isNativeSession(page.session.id)) {
    const row = sessions.find(s => s.machine_id === page.session.machine_id && s.id === page.session.id);
    if (row) keepNativeSession({ ...row, ...page.session });
  }

  const sig = pushed ? `pushed ${read}` : JSON.stringify(page);
  const moved = pushed || sig !== AG.sig;
  if (moved) { AG.sig = sig; AG.changed = Date.now(); }
  if (page.session.question?.id !== AG.answering) AG.answering = null;
  // A session opened with no folder (from a notice) draws again once a read gives it.
  const placed = !AG.page?.session.cwd && !!page.session.cwd;
  // The session closed or was taken over: the composer replaces the banner. A start's banner
  // says what it does now.
  const field = !!AG.page && (followsOnly(AG.page.session) !== followsOnly(page.session)
    || (isStarting(page.session.id) && (AG.page.session.doing !== page.session.doing || AG.page.session.status !== page.session.status)));
  const news = hasNews(AG.page, page);
  // The older entries held stay while the newest are read again.
  const earlier = AG.page?.earlier;
  if (mergePage(page, replace).earlier !== earlier) AG.older.stop = "";
  drawModelPill();
  drawAgentHeading();
  // The frame around the log is drawn again; the log itself stays.
  if (placed) drawAgentFrame(false, sessionBody());
  else if (field && $("#agconv")) drawAgentFrame(true, sessionBody());
  // Back after a lost link, or something new came: earlier errors no longer hold.
  setNote(noteAfterRead({ note: AG.note, failed: AG.readError, link: AG.link, news }));
  AG.readError = "";
  AG.link = LINKED;
  AG.stopping = stopPending(AG.stopping, page.session.id, AGENT_ACTIVE.has(page.session.status), page.turns);
  continueAfterTurn(page.session);
  drawRest(moved, placed);
  finishFirstRead(o, "ok");
  void loadOlder();
}

/// Draws the log again when the page `moved` or what shows with it changed; nothing the log
/// shows changed: no draw.
function drawRest(moved = false, placed = false) {
  const rest = JSON.stringify([openModes(), openTasks(), AG.note, AG.answering, stoppingNow(), AG.page?.queued_messages ?? [], placed]);
  if (!moved && rest === AG.rest) return;
  // Only the page moved: the turns it left as they were keep their rows.
  const news = rest === AG.rest;
  AG.rest = rest;
  drawAgentBody(undefined, true, false, news);
}

/// The session opened again was followed off screen: its model, drawn with
/// the frame, counts as its first read.
function shownFromModel(o: OpenAgent) {
  const read = ++AG.polls;
  AG.sig = `pushed ${read}`;
  AG.changed = Date.now();
  drawAgentHeading();
  // Live: the connecting line under the kept log goes.
  drawRest(true);
  finishFirstRead(o, "ok");
}

// A conversation opened again starts at its latest message. Gestures on
// its previous visit no longer control the scroll.
function followLatest(body: ChatBody | null) {
  if (body) {
    body.stuck = true;
    delete body.wheel;
    delete body.touching;
  }

  return body;
}

// Put a shown log back before a read or a scroll event can mistake a
// hidden view's reset for the reader scrolling up.
function restoreAgentScroll(b = sessionBody()) {
  if (!b) return;
  b.scrollTop = returnScroll(b, b.top ?? 0);
  scrolledLog(b);
  syncJump(b);
}

// A pane shown again resumes its subscription.
window.addEventListener("agent-pane-shown", () => {
  if (S.view === "agent") resumeFollow();
});

void window.__TAURI__.webviewWindow.getCurrentWebviewWindow().listen("center-reopened", () => {
  if (S.view === "agent" && AG.open) restoreAgentScroll(followLatest(sessionBody()));
});

onShownChange(shown => {
  if (AG.open && S.view === "agent") traceSession("visibility-changed", AG.open, { shown });

  if (!shown) { clearTimeout(AG.timer); feed.leave(); }
  else if (S.view === "agent") {
    flushDraw();
    restoreAgentScroll();
    pollAgent();
  }
}, true);

// A phone app back from the background may have been suspended, and its
// links with it: each followed session reads on once from where it was,
// and the open one falls back to a read if its stream does not answer.
onShownChange(shown => {
  if (!shown || !onPhone()) return;
  feed.resync();
  if (S.view === "agent") void pollAgent();
});

/// Shows the title saved for the open session in its head, and in the switcher, which lists it.
/// A followed session pushes only when it works, so the change brings the title.
function retitleOpen(change: Retitled) {
  const shown = AG.page && retitle(AG.page.session, change);
  if (!shown || !AG.page) return;
  setPageSession(shown);
  drawAgentHeading();
}

listen<Retitled>("agent-title-changed", e => retitleOpen(e.payload));

// A notice about a session that asks may mean it was answered elsewhere, and a start says so
// when it waits, starts or fails.
listen("sessions-changed", () => {
  if (S.view === "agent" && (AG.page?.session.status === "asking" || (AG.open && isStarting(AG.open.session)))) pollAgent();
});

// A start's machine named its session: what was kept for the start goes to the session, which
// takes over the view that shows the start.
listen<AgentStarted>("agent-started", ({ payload: { start, session, first_message } }) => {
  setFirstMessage(session, first_message);
  became.set(start, session);
  const pick = sessionPicks.get(start);

  if (pick) {
    sessionPicks.delete(start);
    sessionPicks.set(session.id, pick);
  }

  if (S.view === "agent" && AG.open?.session === start) leaveStart(start);
});

/// The open start `start` is no longer kept: the session it became takes over its view, and one
/// that became none, cancelled from another window, is left as Cancel leaves it.
function leaveStart(start: string) {
  const session = became.get(start);
  if (!session) return agentBack(true);
  // A released node's row leaves out that the session is the assistant.
  const row = AG.page?.session.assistant ? { ...session, assistant: true } : session;
  if (windowHooks) windowHooks.openStarted(row);
  else openAgent(row, { back: AG.back, adopt: true });
}

/// Sends what the field holds: while the turn runs it steers it after its current tool call.
export function submitAgentMessage(input: HTMLTextAreaElement) {
  if (turnRuns()) steerMessage(input);
  else void sendAgentMessage();
}

/// Closes the open session, preserving its conversation for later.
export async function closeOpenSession() {
  const s = AG.page?.session;
  if (!AG.draft && s && AG.open) await closeAgentSession(s);
}

/// Closes a session from its view or the Running list.
export async function closeAgentSession(s: AgentRow) {
  const status = shownStatus(s);
  if (status === "closing" || !sessionOffers({ ...s, status }).close) return;

  const open = S.view === "agent" && !AG.draft && sameSession(AG.open, s) ? AG.open : null;
  const closing = closeSession(s);
  // Leave before the machine acknowledges the close.
  if (open) agentBack(true);

  try {
    await closing;
  } catch (e) {
    if (open && isOpen(open) && paneShown()) {
      setNote(readError(e));
      drawAgentBody();
    } else {
      showFailure(`Could not close session: ${readError(e)}`);
    }
  }
}

/// Retry on the open start, which failed: it waits for its machine again.
async function retryStart() {
  const o = AG.open;
  if (!o) return;

  try {
    await invoke("agent_begin_retry", { session: o.session });
  } catch (e) {
    if (isOpen(o)) setNote(readError(e));
  }

  if (isOpen(o)) void pollAgent(true);
}

