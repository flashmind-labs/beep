import { AG, type AgentPage, type AgentRow } from "./agents.ts";
import { joinRuns, type Run } from "./older-state.ts";
import { endedPage } from "./reconnect-state.ts";
import { sessionKey } from "./conversation-state.ts";

// The one writer of `AG.page`, the open session's page: everything else reads it.

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
  AG.page = withFirstMessage(held ?? { session: s, plan: [], files: [], entries: [], earlier: 0, last_message: "", error: null, queued: 0, queued_messages: [], turns: 0, usage: null });
}

/// No session is open.
export function clearPage() { AG.page = null; }

/// Takes the update `fresh` into the page.
export function mergePage(fresh: AgentPage, replace: boolean): AgentPage {
  return AG.page = heldPage(AG.page, fresh, replace);
}

/// The page kept on disk replaces the empty one, under the session row it opened with, and the
/// steers it held went long ago.
export function adoptKeptPage(kept: AgentPage) {
  if (AG.page) AG.page = { ...kept, session: AG.page.session, queued: 0, queued_messages: [] };
}

/// The older entries `run` read join the page.
export function takeEarlier(run: Run) {
  if (AG.page) AG.page = { ...AG.page, ...run };
}

/// The session row the page shows.
export function setPageSession(s: AgentRow) {
  if (AG.page) AG.page.session = s;
}

/// The session ended with its machine: no turn runs on.
export function endPage() {
  if (AG.page) AG.page = endedPage(AG.page);
}

/// The open question was answered.
export function clearQuestion() {
  if (!AG.page) return;
  AG.page.session.question = null;
  AG.page.session.status = "working";
}

/// A message just sent shows in the log before its machine logs it.
export function appendEntry(page: AgentPage, entry: AgentPage["entries"][number]) {
  page.entries.push(entry);
}

/// Takes out `entry`, not the last, as a read may have drawn more since.
export function removeEntry(page: AgentPage, entry: AgentPage["entries"][number]) {
  const at = page.entries.indexOf(entry);
  if (at >= 0) page.entries.splice(at, 1);
}

export function dropLastEntry(page: AgentPage) {
  page.entries.pop();
}
