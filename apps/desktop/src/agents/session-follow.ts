// The sessions followed by stream: the open one and, off screen, those that
// run or ask, and the pushed updates held until the next frame.
import { AgentFeed, type Activity } from "./agentfeed-state.ts";
import { transcripts, transcriptKey, type TranscriptCursor, type TranscriptUpdate } from "./transcript-cache-state.ts";
import { S, changeListeners } from "../bar/core.ts";
import { type AgentRow, type AgentPage, type OpenAgent, type AgentModes, type AgentTask, AG, AGENT_ACTIVE, where } from "./agents-state.ts";
import { drawAgentBody } from "./logview.tsx";
import { disposeQueue } from "./messages.ts";
import { Channel, invoke } from "../ui/tauri.ts";
import { joinRuns, viewRead } from "./older-state.ts";
import { readFailed } from "./reconnect-state.ts";
import { mergePage } from "./open-page.ts";
import { isOpen, setReadNote, wantRead } from "./open-state.ts";
import { signal } from "../ui/signal.ts";
import { paneShown } from "../ui/active.ts";
import { publishTurn } from "../plugins/plugins-state.ts";

/// A pushed update of the open session `open`, due to be shown: its page applies it.
export const pushedPage = signal<{ open: OpenAgent; page: AgentPage; read: number; replace: boolean }>();

interface FeedUpdate extends TranscriptUpdate<AgentPage> {
  modes: AgentModes | null;
  tasks: AgentTask[];
}

/// What a session is doing, for how long it stays followed off screen.
const activityOf = (s: AgentRow): Activity =>
  s.question || s.status === "asking" ? "asking" : AGENT_ACTIVE.has(s.status) ? "running" : "idle";

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
    return invoke<void>("agent_follow", { ...where(o), id, last: viewRead(window.innerHeight), after: transcripts.get(transcriptKey(o))?.cursor ?? null, onEvent });
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
        wantRead(true);
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
    wantRead(false);
  },
  // A node without the follow, or one that failed to open it: the shown session is read instead.
  failed: o => { if (isOpen(o) && paneShown()) wantRead(false); },
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

  if (isOpen(next.open)) pushedPage.notify({ open: next.open, page: next.page, read: next.read, replace: next.replace });
  else if (AG.open === next.open && AG.page) mergePage(next.page, next.replace);
}

/// The open session leaves the screen; it stays followed while it runs or asks.
export function stopAgentFollow() {
  clearTimeout(AG.timer);
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
  feed.clear();
}

changeListeners.push(() => {
  // What was held joins the model now: a later update must not come before it.
  if (S.view !== "agent") { flushDraw(true); stopAgentFollow(); disposeQueue(); }
});
