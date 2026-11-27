// The sessions followed by stream: the open one and, off screen, those that
// run or ask, and the pushed updates held until the next frame.
import { createStore } from "zustand/vanilla";
import { AgentFeed, type Activity } from "./agentfeed-state.ts";
import { transcripts, transcriptKey, type TranscriptCursor, type TranscriptUpdate } from "./transcript-cache-state.ts";
import { $, PANE, S, changeListeners } from "../bar/core.ts";
import { currentLocation, go } from "../center/nav.ts";
import { type AgentRow, type AgentPage, type OpenAgent, type AgentModes, type AgentTask, AG, AGENT_ACTIVE, AGENT_LAST, AGENT_READ_WAIT_MS, openModes, openTasks, where } from "./agents-state.ts";
import { drawAgentBody } from "./logview.tsx";
import { disposeQueue } from "./composer-ui.tsx";
import { Channel, invoke, listen, logError, logInfo, recordTiming } from "../ui/tauri.ts";
import { joinRuns, newestRead, viewRead } from "./older-state.ts";
import { hasNews, LINKED, noteAfterRead, readFailed, RETRY_MS, viewError } from "./reconnect-state.ts";
import { endPage, mergePage } from "./open-page.ts";
import { followStale, followsOnly, isOpen, setNote, setReadNote, openSession, staleFrame, stoppingNow } from "./open-state.ts";
import { sessionBody } from "./session-frame-state.ts";
import { isNativeSession, nativeUpdate } from "./native-session-state.ts";
import { keepNativeSession, sessions, sessionsRead } from "./sessions.ts";
import { conversationHistory, openOf } from "./conversation-state.ts";
import { isStarting } from "./starting-state.ts";
import { loadOlder } from "./older.ts";
import { readModes } from "./modepill.tsx";
import { readTasks } from "./tasks.tsx";
import { drawModelPill } from "./modelpill.tsx";
import { drawAgentHeading } from "./session-head.tsx";
import { stopPending } from "./agentqueue.ts";
import { continueAfterTurn } from "./continue-here.ts";

import { paneShown, visibilitySnapshot } from "../ui/active.ts";
import { onPhone } from "../ui/platform.ts";
import { publishTurn } from "../plugins/plugins-state.ts";

interface FeedUpdate extends TranscriptUpdate<AgentPage> {
  modes: AgentModes | null;
  tasks: AgentTask[];
}

/// What a session is doing, for how long it stays followed off screen.
const activityOf = (s: AgentRow): Activity =>
  s.question || s.status === "asking" ? "asking" : AGENT_ACTIVE.has(s.status) ? "running" : "idle";

/// The cursor a follow goes on from. A page with no fingerprint has none to match, and any `after`
/// makes the node read its whole tail, so it asks for the newest page instead.
export const followFrom = (cursor: TranscriptCursor | undefined): TranscriptCursor | null => cursor?.fingerprint ? cursor : null;

/// Follows the open session and, off screen, those that run or ask, so they
/// show at once when opened again. Off screen an update only joins its model.
export const feed = new AgentFeed<OpenAgent, FeedUpdate>({
  id: () => crypto.randomUUID(),
  now: () => Date.now(),
  name: transcriptKey,
  later: (ms, run) => { const timer = setTimeout(run, ms); return () => clearTimeout(timer); },
  show: id => { void invoke("agent_follow_show", { id }).catch(e => console.warn("session shown:", e)); },
  open: (o, id, receive) => {
    const onEvent = new Channel<FeedUpdate>();
    onEvent.onmessage = receive;
    return invoke<void>("agent_follow", { ...where(o), id, last: viewRead(window.innerHeight), after: followFrom(transcripts.get(transcriptKey(o))?.cursor), onEvent });
  },
  close: id => { void invoke("agent_unfollow", { id }).catch(e => console.warn("session detach:", e)); },
  // What it said of its modes and tasks goes stale unless it is the open session, read again after a gap.
  unfollowed: o => { if (!AG.open || transcriptKey(AG.open) !== transcriptKey(o)) transcripts.clearExtras(transcriptKey(o)); },
  receive: (o, update) => {
    const key = transcriptKey(o);
    const page = transcripts.receive(key, update);
    const open = AG.open && transcriptKey(AG.open) === key ? AG.open : null;

    // A gap: the session is read again whole, now if it shows, else when it opens.
    if (!page) {
      feed.drop(o);

      if (isOpen(open)) {
        flushDraw(true);
        void pollAgent(true);
      }

      return;
    }

    feed.activity(o, activityOf(page.session));
    publishTurn(o.machine_id, page);
    const fresh = update.kind === "delta" ? update.page : page;
    const replace = update.kind === "snapshot" && !update.resumed;
    if (!open) return;

    // Left for another view: its model takes what was held, then the
    // update, with no draw, so updates join in the order they came.
    if (!isOpen(open)) {
      flushDraw(true);
      if (AG.page) mergePage(fresh, replace);
      return;
    }

    // Normal updates only redraw their changed suffix.
    queueDraw(open, fresh, replace);
  },
  disconnected: (o, error) => {
    if (!isOpen(o) || !paneShown()) return;
    console.warn("session stream disconnected:", error);
    const failed = readFailed(AG.link, { kind: "lost" }, AG.page?.session.machine_name || "the machine");
    AG.link = failed.link;
    setReadNote(failed.note);
    drawAgentBody();
    void pollAgent(false);
  },
  // A node without the follow, or one that failed to open it: the shown session is read instead.
  failed: o => { if (isOpen(o) && paneShown()) void pollAgent(false); },
});

/// What the app kept of a session, as `transcript_kept` gives it.
interface KeptTranscript {
  page: AgentPage;
  cursor: TranscriptCursor | null;
}

/// The page kept on disk for `o`, held so the follow goes on from its cursor; null when none was
/// kept or another page is held already.
export async function keptTranscript(o: OpenAgent): Promise<AgentPage | null> {
  const kept = await invoke<KeptTranscript | null>("transcript_kept", where(o)).catch(e => { console.warn("kept transcript:", e); return null; });
  const key = transcriptKey(o);
  if (!kept || transcripts.peek(key)) return null;
  if (kept.cursor) transcripts.seed(key, { page: kept.page, cursor: kept.cursor });
  return kept.page;
}

/// How many sessions are made ready before they are opened.
const WARM_SESSIONS = 4;
/// What each session warmed was doing when it was tried, by key. A session is tried again only when
/// that changes, so a failed follow or a kept read is not repeated by every push of the list.
const tried = new Map<string, string>();

/// Makes ready the sessions the user most likely opens next, so each draws at the bottom at once:
/// those asking, then those running, then the latest visited. Each takes its page kept on disk; one
/// that asks or runs is then followed off screen, from the cursor of that page when it has one. An
/// idle one is not read over the mesh before a click.
export function warmSessions(all: readonly AgentRow[]) {
  if (!paneShown() || onPhone()) return;
  // A start has no stream to follow.
  const rows = conversationHistory.order(all, null).filter(r => r.status !== "starting" && !isStarting(r.id));
  const asking = rows.filter(r => activityOf(r) === "asking");
  const running = rows.filter(r => activityOf(r) === "running");
  const visited = conversationHistory.visited(rows);
  const picked = new Map<string, AgentRow>();

  for (const row of [...asking, ...running, ...visited]) {
    if (picked.size >= WARM_SESSIONS) break;
    picked.set(transcriptKey(openOf(row)), row);
  }

  for (const key of [...tried.keys()]) if (!picked.has(key)) tried.delete(key);

  for (const [key, row] of picked) {
    const state = `${row.machine_id} ${row.status} ${activityOf(row)}`;
    if (tried.get(key) === state) continue;
    tried.set(key, state);
    const o = openOf(row);
    // The row as it is once the kept page is read: it may have gone idle since.
    const follow = () => {
      const now = sessions.find(s => s.machine_id === row.machine_id && s.id === row.id);
      const activity = now ? activityOf(now) : "idle";
      if (activity !== "idle" && paneShown()) feed.warm(o, activity);
    };

    if (transcripts.peek(key)) follow();
    else void keptTranscript(o).then(follow);
  }
}

sessionsRead.add(() => warmSessions(sessions));
// A link that dropped closed the readers off screen: the next list warms them again.
listen("reconnected", () => tried.clear());

/// The steers the live page lists. It must still be followed for its queue to be current.
export function heldSteers(o: OpenAgent): string[] {
  return feed.live(o) ? transcripts.peek(transcriptKey(o))?.page.queued_messages ?? [] : [];
}

/// The node dropped its steers after Stop all.
export function clearHeldSteers(o: OpenAgent) {
  transcripts.amend(transcriptKey(o), page => ({ ...page, queued: 0, queued_messages: [] }));
}

/// Drops what is held of a deleted session, in memory and on disk.
export function forgetTranscript(o: OpenAgent) {
  const key = transcriptKey(o);
  transcripts.delete(key);
  void invoke("transcript_forget", where(o)).catch(e => console.warn("forgetting the transcript:", e));
}

/// Pushed updates held for the next frame, merged in the order they came.
/// `read` numbers the newest, as a read before steers sent after it.
let held: { open: OpenAgent; page: AgentPage; replace: boolean; read: number; frame: number } | null = null;

/// Draws a burst of pushed updates once per frame: each delta joins the
/// held page in order, and a snapshot replaces it. While the view is
/// hidden they are held, with no frame asked, until it shows again.
function queueDraw(o: OpenAgent, page: AgentPage, replace: boolean) {
  if (held && held.open !== o) flushDraw(true);

  if (held) {
    held.page = replace ? page : joinRuns(held.page, page);
    held.replace ||= replace;
    held.read = ++AG.polls;
  } else {
    held = { open: o, page, replace, read: ++AG.polls, frame: 0 };
  }

  if (!held.frame && paneShown()) held.frame = requestAnimationFrame(() => flushDraw());
}

/// Applies the held updates, on screen or `now`; a hidden view keeps them
/// for when it shows. A session no longer shown keeps them in its model.
export function flushDraw(now = false) {
  const next = held;
  if (!next) return;
  cancelAnimationFrame(next.frame);
  next.frame = 0;
  if (!now && isOpen(next.open) && !paneShown()) return;
  held = null;

  if (isOpen(next.open)) applyAgentPage(next.open, next.page, next.read, next.replace, true);
  else if (AG.open === next.open && AG.page) mergePage(next.page, next.replace);
}

/// Stops following `o` and discards any frame queued from its old stream.
export function dropFollow(o: OpenAgent) {
  if (held && transcriptKey(held.open) === transcriptKey(o)) {
    cancelAnimationFrame(held.frame);
    held = null;
  }
  feed.drop(o);
}

/// The open session leaves the screen; it stays followed while it runs or asks.
export function stopAgentFollow() {
  clearTimeout(AG.timer);
  pollAgain = null;
  feed.leave();
  forgetUnfollowedExtras(AG.open);
}

/// A session read by polling has no follow to end and clear its modes and tasks, so they go as it
/// leaves the screen, unless it is followed still.
export function forgetUnfollowedExtras(o: OpenAgent | null) {
  if (o && !feed.live(o)) transcripts.clearExtras(transcriptKey(o));
}

/// Stops following every session, the open one included.
export function forgetAgentFollows() {
  clearTimeout(AG.timer);
  pollAgain = null;
  feed.clear();
}

changeListeners.push(() => {
  // What was held joins the model now: a later update must not come before it.
  if (S.view !== "agent") { flushDraw(true); stopAgentFollow(); disposeQueue(); }
});

/// The session each start kept on this device became, by the start's id: a page that still
/// names the start opens the session.
export const became = new Map<string, AgentRow>();

let pollAgain: { open: OpenAgent; readNow: boolean } | null = null;

/// Asks for the open session, and opens its follow again after `RETRY_MS`
/// while it stays on screen without one. Opening reads once even if the webview's
/// visibility has not caught up. Never two calls at once for one session.
export async function pollAgent(readNow = false) {
  clearTimeout(AG.timer);
  const o = AG.open;
  if (!o) { pollAgain = null; feed.leave(); return; }
  const trace = readNow || !AG.sig;
  const skip = !isOpen(o) ? "different-view" : AG.inflight === o || track.kept === o ? "in-flight" : !readNow && !paneShown() ? "hidden" : null;

  if (skip) {
    if (AG.inflight === o) pollAgain = { open: o, readNow: (pollAgain?.open === o && pollAgain.readNow) || readNow };
    if (skip !== "in-flight") feed.leave();
    if (trace) traceSession("read-skipped", o, { reason: skip, readNow });
    return;
  }

  if (pollAgain?.open === o) pollAgain = null;

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
  if (AG.open !== o) { pollAgain = null; if (isOpen(AG.open) && !AG.inflight) void pollAgent(); return; }

  if (pollAgain?.open === o || (!still() && !feed.live(o))) {
    const again = pollAgain?.open === o && pollAgain.readNow;
    pollAgain = null;
    if (isOpen(o) && (again || paneShown())) void pollAgent(again);
    return;
  }

  if (!isOpen(o) || !paneShown()) {
    if (trace) traceSession("polling-stopped", o, { reason: !isOpen(o) ? "different-view" : "hidden" });
    return;
  }

  // The follow is opened again every `RETRY_MS`. A start says when it changes (`sessions-changed`), and an ended session does not change.
  const quiet = isStarting(o.session) || AG.page?.session.status === "ended";
  if (!feed.live(o) && (AG.link.lost || !quiet)) AG.timer = setTimeout(pollAgent, RETRY_MS);
}

/// What a session's first read and its kept page are doing: when the read began, the session whose
/// page kept on disk is being read (its follow starts after, from its cursor), and the timing of
/// a kept page to its first fresh read.
export const track: { first: { open: OpenAgent; at: number } | null; kept: OpenAgent | null; fresh: { open: OpenAgent; done: () => void } | null } = { first: null, kept: null, fresh: null };
function finishFirstRead(open: OpenAgent, outcome: "ok" | "error") {
  if (track.fresh?.open === open) { if (outcome === "ok") track.fresh.done(); track.fresh = null; }
  if (!track.first || track.first.open !== open) return;
  recordTiming("data", "agent", performance.now() - track.first.at, outcome);
  track.first = null;
}

/// A session opened that still has no read after this long leaves a line in the log.
const READ_WATCH_MS = 30_000;

let reading: { open: OpenAgent; at: number; pending: Set<string> } | null = null;

export function traceSession(stage: string, o: OpenAgent, details: Record<string, unknown> = {}) {
  logInfo(`session view: ${JSON.stringify({ stage, session: o.session, machine: o.machine_id, view: S.view, body: !!sessionBody(), ...visibilitySnapshot(), ...details })}`);
}

/// Logs it when the session `o` opened still shows nothing read after
/// `READ_WATCH_MS` with no error said: the view is stuck, and why (#427).
export function watchRead(o: OpenAgent) {
  setTimeout(() => {
    if (AG.open !== o || !isOpen(o) || AG.sig || AG.readError) return;
    logError(`session view: nothing read for session ${o.session} after ${READ_WATCH_MS / 1000} s ${JSON.stringify({ reading: AG.inflight === o, pending: reading?.open === o ? [...reading.pending] : [], view: S.view, body: !!sessionBody(), ...visibilitySnapshot() })}`);
  }, READ_WATCH_MS);
}


/// A `pushed` update always holds something new; a polled one is compared
/// with the last read.
function applyAgentPage(o: OpenAgent, page: AgentPage, read: number, replace = false, pushed = false) {
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
  if (placed) staleFrame(false);
  else if (field && $("#agconv")) staleFrame(true);
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


/// The open start `start` is no longer kept: the session it became takes over its view, and one
/// that became none, cancelled from another window, is left as Cancel leaves it.
/// How many open starts ended with no session: the page leaves their view, as Cancel does.
export const startLost = createStore<{ lost: number }>(() => ({ lost: 0 }));

export function leaveStart(start: string) {
  const session = became.get(start);
  if (!session) return startLost.setState(s => ({ lost: s.lost + 1 }));
  // A released node's row leaves out that the session is the assistant.
  const row = AG.page?.session.assistant ? { ...session, assistant: true } : session;
  // The window shows the session in the page that shows the start; nothing happens while it shows another page.
  if (!PANE) openSession(row, { back: AG.back, adopt: true });
  else if (currentLocation()?.view === "pane") go("pane", { agent: row, adopt: true });
}


let followed = 0;
followStale.subscribe(({ stale }) => {
  if (stale === followed) return;
  followed = stale;
  void pollAgent(false);
});
