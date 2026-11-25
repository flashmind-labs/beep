// How the open session's view rides out a dropped link to its machine: a
// muted line while it reads again every few seconds, gone once the session reads
// again or anything new arrives, and a plain ending when the machine came
// back without it.
import type { AgentPage } from "./agents-state.ts";

/// Why `agent_view` failed, as the app tells it.
type ViewError = { kind: "lost" } | { kind: "gone" } | { kind: "failed"; message: string };

/// Reads lost in a row, and whether the session ended with its machine.
export interface Link { lost: number; ended: boolean }
export const LINKED: Link = { lost: 0, ended: false };

/// The pause before reaching a machine again after a link to it dropped,
/// as the app's own links wait (`beep_mesh::RETRY`).
export const RETRY_MS = 5000;

/// What a rejected `agent_view` said; anything else is a failure to show.
export function viewError(e: unknown): ViewError {
  const kind = typeof e === "object" && e ? (e as { kind?: unknown }).kind : undefined;
  if (kind === "lost" || kind === "gone") return { kind };
  if (kind === "failed") return { kind, message: String((e as { message?: unknown }).message ?? "") };
  return { kind: "failed", message: String(e) };
}

export const lostNote = (machine: string) => `Lost the connection to ${machine}. Reconnecting...`;
/// Under a log drawn from what this device kept, until the first live read or event.
export const connectingNote = (machine: string) => `Connecting to ${machine}...`;

/// Why `agent_steer` failed: the machine could not be reached ("lost"), the
/// link dropped once it went ("unsure"), the message waits in this device's
/// outbox until the machine answers ("waiting"), the page stopped waiting
/// for an answer ("timeout"), or a refusal.
export interface SendError { kind: "lost" | "unsure" | "waiting" | "timeout" | "failed"; message: string }

/// Whether `invoke` gave up on a command after its limit: it rejects with
/// "no answer in N s. Try again.".
const timedOut = (e: unknown) => typeof e === "string" && /^no answer in \d+ s\./.test(e);

export function sendError(e: unknown): SendError {
  if (timedOut(e)) return { kind: "timeout", message: String(e) };
  const o = typeof e === "object" && e ? e as { kind?: unknown; message?: unknown } : null;
  const kind = o?.kind === "lost" || o?.kind === "unsure" || o?.kind === "waiting" ? o.kind : "failed";
  return { kind, message: String(o ? o.message ?? "" : e).replace(/^bad request: /, "") };
}

/// After a failed read: the link, and the note under the log ("gone" for a
/// session the machine does not have, with no lost link before it).
export function readFailed(link: Link, e: unknown, machine: string): { link: Link; note: string } {
  const v = viewError(e);
  if (v.kind === "lost") return { link: { lost: link.lost + 1, ended: false }, note: lostNote(machine) };
  if (v.kind === "gone" && (link.lost > 0 || link.ended)) return { link: { lost: 0, ended: true }, note: `This session ended when ${machine} restarted.` };
  if (v.kind === "gone") return { link: LINKED, note: "gone" };
  return { link: LINKED, note: v.message };
}

/// The page once its session ended with its machine: no turn runs on.
export function endedPage(page: AgentPage): AgentPage {
  return page.session.status === "ended" ? page : { ...page, session: { ...page.session, status: "ended", question: null } };
}

type Seen = Pick<AgentPage, "earlier" | "entries"> & { session: Pick<AgentPage["session"], "status"> };

/// Whether `fresh` brings something `held` did not have: a new log line or
/// another status.
export function hasNews(held: Seen | null, fresh: Seen) {
  if (!held) return true;
  return fresh.earlier + fresh.entries.length > held.earlier + held.entries.length || fresh.session.status !== held.session.status;
}

/// The note left under the log after a read that got the session. What a
/// failed read said goes, and so does any error once the link came back or
/// something new arrived: it no longer holds.
export function noteAfterRead(o: { note: string; failed: string; link: Link; news: boolean }): string {
  if (!o.note || o.note === "gone" || o.note === o.failed || o.link.lost > 0 || o.link.ended || o.news) return "";
  return o.note;
}
