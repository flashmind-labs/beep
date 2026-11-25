import type { AgentPage, AgentRow } from "./agents-state.ts";
import { joinRuns, type Run } from "./older-state.ts";
import { endedPage } from "./reconnect-state.ts";
import { sessionKey } from "./conversation-state.ts";

// The open session's page, with the one place that changes it. `AG.page` reads it. A page is
// replaced, never changed in place, as `transcripts` may hold the same object. This module
// imports no views, so the modules that write the page can import it without a cycle.

let page: AgentPage | null = null;

/// The open session's page, null when none is open.
export const currentPage = () => page;

/// The message each session was started with, by session, until its machine logs one.
const firstMessages = new Map<string, string>();

export function setFirstMessage(s: { machine_id: string; id: string }, text: string) {
  firstMessages.set(sessionKey(s), text);
}

/// `page` with the message its session was started with, while its machine has logged nothing.
function withFirstMessage(page: AgentPage): AgentPage {
  const key = sessionKey(page.session), first = firstMessages.get(key);
  if (!first) return page;

  if (page.entries.length || page.earlier) {
    firstMessages.delete(key);
    return page;
  }

  return { ...page, entries: [{ kind: "user", text: first }] };
}

/// The page to hold once `fresh` came: joined on `held`, or alone when it replaces it, with the
/// message its session was started with while its machine has logged nothing.
function heldPage(held: AgentPage | null, fresh: AgentPage, replace: boolean): AgentPage {
  const page = withFirstMessage(fresh);
  return replace ? page : joinRuns(held, page);
}

/// Opens `s` on `held`, or on an empty page.
export function openPage(s: AgentRow, held: AgentPage | null) {
  page = withFirstMessage(held ?? { session: s, plan: [], entries: [], earlier: 0, error: null, queued: 0, queued_messages: [], turns: 0, usage: null });
}

/// No session is open.
export function clearPage() { page = null; }

/// Puts `next` in place, for `AG.page = ...` where a test sets the scene.
export function setPage(next: AgentPage | null) { page = next; }

/// Takes the update `fresh` into the page.
export function mergePage(fresh: AgentPage, replace: boolean): AgentPage {
  return page = heldPage(page, fresh, replace);
}

/// The page kept on disk replaces the empty one, under the session row it opened with, and the
/// steers it held went long ago.
export function adoptKeptPage(kept: AgentPage) {
  if (page) page = { ...kept, session: page.session, queued: 0, queued_messages: [] };
}

/// The older entries `run` read join the page.
export function takeEarlier(run: Run) {
  if (page) page = { ...page, ...run };
}

/// The session row the page shows.
export function setPageSession(s: AgentRow) {
  if (page) page = { ...page, session: s };
}

/// The session ended with its machine: no turn runs on.
export function endPage() {
  if (page) page = endedPage(page);
}

/// The open question was answered.
export function clearQuestion() {
  if (page) page = { ...page, session: { ...page.session, question: null, status: "working" } };
}

/// A message just sent shows in the log before its machine logs it.
export function appendEntry(entry: AgentPage["entries"][number]) {
  if (page) page = { ...page, entries: [...page.entries, entry] };
}

/// Takes out `entry`, not the last, as a read may have drawn more since.
export function removeEntry(entry: AgentPage["entries"][number]) {
  if (page?.entries.includes(entry)) page = { ...page, entries: page.entries.filter(en => en !== entry) };
}

/// Takes out the entry `appendEntry` put last.
export function dropLastEntry() {
  if (page) page = { ...page, entries: page.entries.slice(0, -1) };
}
