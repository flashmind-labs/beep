// One coding agent session in the bar: opening it, drawing its frame and
// reading it while it shows. Following it is in session-follow.ts, its head
// in session-head.ts and what a click in it does in session-click.ts.
import { disposeQueue } from "./composer-ui.tsx";
import { SessionField, SessionHead } from "./session-frame.tsx";
import { agentMessage, sessionBody, showSessionFrame } from "./session-frame-state.ts";
import { followsOnly, isOpen, keepLog, frameStale, keptLogs, openSession, resumeFollow, openState, sessionPicks, setNote, turnRuns, type KeptLog } from "./open-state.ts";
import { bindSessionStash } from "./stash.ts";
import { transcriptKey, transcripts } from "./transcript-cache-state.ts";
import { agentDrop } from "../chat/attach.tsx";
import { fileLinkMenu } from "./filelinks.ts";
import { agentKey, conversationHistory, conversationKey, retitle, sessionKey, type Retitled } from "./conversation-state.ts";

import { $, type AgentReturn, bar, S, windowHooks, setView } from "../bar/core.ts";
import { showFailure } from "../ui/failbanner-view.tsx";
import { openLinked, rebindPane, restorePane, startPane } from "../agentpane/media.ts";
import { followsAfterScroll, pinWait, returnScroll } from "./logscroll.ts";
import { bindMentionMenu } from "./mentionmenu.ts";
import { MN, activateMentions, rememberMentionImages, setAttachError } from "./mentions.ts";
import { agentBack } from "./agents-list.ts";
import { type AgentRow, type AgentPage, type OpenAgent, AG, sessionUsage, canTakeOver, openOf, where } from "./agents-state.ts";
import { agentKind } from "./agent-kind.ts";
import { agentAttachError, drawAgentAttached, removeLastAttachment } from "./attachments-view.tsx";
import { sessions, sessionsRead } from "./sessions.ts";
import { closeSession, shownStatus } from "./closing.ts";
import { sessionOffers } from "./sessionmenu-state.ts";
import { drawAgentBody, syncJump, scrolledLog } from "./logview.tsx";
import { drawModelPill } from "./modelpill.tsx";


import { interruptAgent, readOutbox, sendAgentMessage, sendPrompt, steerMessage } from "./messages.ts";
import type { ChatBody } from "../ui/turn.ts";
import { invoke, listen } from "../ui/tauri.ts";
import { perfFresh } from "../ui/perf.ts";
import { onShownChange, paneShown } from "../ui/active.ts";
import { onPhone } from "../ui/platform.ts";
import { readError } from "../agentpane/read-file.ts";

import { joinRuns } from "./older-state.ts";
import { adoptKeptPage, openPage, setFirstMessage, setPageSession } from "./open-page.ts";
import { LINKED } from "./reconnect-state.ts";
import { drawMove } from "./move.ts";
import { moveStore, refreshMoves } from "./move-store.ts";
import { movedTo } from "./move-state.ts";
import { sameSession } from "./running-state.ts";
import { isNativeSession } from "./native-session-state.ts";
import { type AgentStarted, isStarting } from "./starting-state.ts";
import { became, feed, flushDraw, forgetUnfollowedExtras, keptTranscript, leaveStart, pollAgent, track, traceSession, watchRead } from "./session-follow.ts";
import { drawAgentHeading, headText, redrawAgentHints, agentHints } from "./session-head.tsx";
import { agentClick, openParent, toolKey } from "./session-click.ts";
import { startLost } from "./session-follow.ts";
import { askContinueHere, keepContinuing } from "./continue-here.ts";

/// Opens session `s` (a session row) and follows it while it is on screen. With `adopt`,
/// the empty view that started it, or the start it was until its machine named it, becomes
/// the session in place: its pane, terminal tabs included, stays as it is.
function showSession(s: AgentRow, { why = "", back: from, adopt = false }: { why?: string; back?: AgentReturn; adopt?: boolean } = {}) {
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
    if (AG.open) track.first = { open: AG.open, at: performance.now() };
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
  track.first = { open, at: performance.now() };
  drawAgentFrame(inPlace, followLatest(held && log && held.earlier <= log.page.earlier ? log.body : null));
  watchRead(open);
  // A start has no outbox and no kept page.
  if (isStarting(s.id)) return void pollAgent(true);
  void readOutbox(open);
  if (held) return void pollAgent(true);
  // Not held in this window: the page kept on disk draws at once, and the follow goes on from it.
  track.kept = open;
  void keptTranscript(open).then(page => {
    if (track.kept === open) track.kept = null;
    if (AG.open !== open) return;
    // The session row it opened with is newer than the kept one, and the steers it held went long ago.
    if (page && !AG.sig && AG.page) {
      adoptKeptPage(page);
      if (isOpen(open)) {
        drawAgentHeading();
        drawAgentBody();
        track.fresh = { open, done: perfFresh("session") };
      }
    }

    void pollAgent(true);
  });
}

// The page shows the session `openState` names, each time it is asked for.
openState.subscribe(({ row, why, back, adopt }) => {
  if (row) showSession(row, { why, back, adopt });
});
startLost.subscribe(() => agentBack(true));
// What the read found changed the frame around the log: it is drawn again, the log kept.
frameStale.subscribe(({ inPlace }) => drawAgentFrame(inPlace, sessionBody()));

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

// The open session moved and its new machine lists it: the page follows it there.
sessionsRead.add(() => {
  if (S.view !== "agent" || !AG.open) return;
  const there = movedTo(AG.open, moveStore.getState().records, sessions);
  if (there) openSession(there, { back: AG.back });
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

