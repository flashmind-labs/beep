// The sessions followed by stream: the open one and, off screen, those that
// run or ask, and the pushed updates held until the next frame.
import { AgentFeed, BACKGROUND_FOLLOWS, type Activity } from "./agentfeed-state.ts";
import { TranscriptCache, transcriptKey, type TranscriptCursor, type TranscriptUpdate } from "./transcript-cache-state.ts";
import { S, changeListeners } from "../bar/core.ts";
import { type AgentRow, type AgentPage, type OpenAgent, AG, AGENT_ACTIVE, where } from "./agents.ts";
import { drawAgentBody } from "./logview.ts";
import { disposeQueue } from "./messages.ts";
import { Channel, invoke } from "../ui/tauri.ts";
import { ambient, onShownChange } from "../ui/active.ts";
import { joinRuns } from "./older-state.ts";
import { readFailed } from "./reconnect-state.ts";
import { agentOnScreen, applyAgentPage, isOpen, pollAgent } from "./session.ts";

// Room for the open session, those followed off screen and a few recent.
export const transcripts = new TranscriptCache<AgentPage>(BACKGROUND_FOLLOWS + 4);

interface FeedUpdate extends TranscriptUpdate<AgentPage> {
  modes: typeof AG.modes;
  tasks: typeof AG.tasks;
}

/// The newest modes and tasks of each followed session, by `transcriptKey`, for when it opens
/// again and for its menu.
export const offScreen = new Map<string, Pick<FeedUpdate, "modes" | "tasks">>();

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
    return invoke<void>("agent_follow", { ...where(o), id, last: 200, after: transcripts.get(transcriptKey(o))?.cursor ?? null, onEvent });
  },
  close: id => { void invoke("agent_unfollow", { id }).catch(e => console.warn("session detach:", e)); },
  unfollowed: o => { offScreen.delete(transcriptKey(o)); },
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
    unkept.set(key, o);
    keepSoon();
    offScreen.set(key, { modes: update.modes, tasks: update.tasks });
    const fresh = update.kind === "delta" ? update.page : page;
    if (!open) return;

    AG.modes = update.modes;
    AG.tasks = update.tasks;

    // Left for another view: its model takes what was held, then the
    // update, with no draw, so updates join in the order they came.
    if (!isOpen(open)) {
      flushDraw(true);
      if (AG.page) AG.page = update.kind === "snapshot" ? page : joinRuns(AG.page, fresh);
      return;
    }

    // Normal updates only redraw their changed suffix.
    queueDraw(open, fresh, update.kind === "snapshot");
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
  // A node without the follow, or one that failed to open it: the shown session is read instead.
  failed: o => { if (isOpen(o) && agentOnScreen()) void pollAgent(); },
});

/// How often, at most, a session that streams has its newest page kept on disk.
const KEEP_MS = 5000;
/// Sessions whose newest page changed since it was last kept on disk.
const unkept = new Map<string, OpenAgent>();

/// Keeps each changed session's newest page and its cursor on disk, so it draws at once after
/// a restart or in another window, then follows on from the cursor.
export function keepTranscripts() {
  for (const [key, o] of unkept) {
    const held = transcripts.peek(key);
    if (held) void invoke("transcript_keep", { ...where(o), page: held.page, cursor: held.cursor }).catch(e => console.warn("keeping the transcript:", e));
  }

  unkept.clear();
}

const keepSoon = ambient(keepTranscripts, KEEP_MS);
// Kept as the session view leaves the screen: the window hides or the pane goes.
onShownChange(shown => { if (!shown) keepTranscripts(); }, true);

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

/// The steers the node holds for `o`, as its newest page says, now dropped by a Stop all: the
/// page held no longer lists them.
export function takeHeldSteers(o: OpenAgent): string[] {
  const held = transcripts.peek(transcriptKey(o));
  if (!held) return [];
  const steers = held.page.queued_messages ?? [];
  held.page = { ...held.page, queued: 0, queued_messages: [] };
  return steers;
}

/// Drops what is held of a deleted session, in memory and on disk.
export function forgetTranscript(o: OpenAgent) {
  const key = transcriptKey(o);
  transcripts.delete(key);
  unkept.delete(key);
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

  if (!held.frame && agentOnScreen()) held.frame = requestAnimationFrame(() => flushDraw());
}

/// Applies the held updates, on screen or `now`; a hidden view keeps them
/// for when it shows. A session no longer shown keeps them in its model.
export function flushDraw(now = false) {
  const next = held;
  if (!next) return;
  cancelAnimationFrame(next.frame);
  next.frame = 0;
  if (!now && isOpen(next.open) && !agentOnScreen()) return;
  held = null;

  if (isOpen(next.open)) applyAgentPage(next.open, next.page, next.read, next.replace, true);
  else if (AG.open === next.open && AG.page) AG.page = next.replace ? next.page : joinRuns(AG.page, next.page);
}

/// The open session leaves the screen; it stays followed while it runs or asks.
export function stopAgentFollow() {
  clearTimeout(AG.timer);
  feed.leave();
}

/// Stops following every session, the open one included.
export function forgetAgentFollows() {
  clearTimeout(AG.timer);
  feed.clear();
  offScreen.clear();
}

changeListeners.push(() => {
  // What was held joins the model now: a later update must not come before it.
  if (S.view !== "agent") { flushDraw(true); stopAgentFollow(); disposeQueue(); keepTranscripts(); }
});
