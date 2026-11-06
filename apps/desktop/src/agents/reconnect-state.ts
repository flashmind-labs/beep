// How the open session's view rides out a dropped link to its machine: a
// muted line while it reads again with backoff, gone once the session reads
// again or anything new arrives, and a plain ending when the machine came
// back without it.
import type { AgentPage } from "./agents.ts";

/// Why `agent_view` failed, as the app tells it.
export type ViewError = { kind: "lost" } | { kind: "gone" } | { kind: "failed"; message: string };

/// Reads lost in a row, and whether the session ended with its machine.
export interface Link { lost: number; ended: boolean }
export const LINKED: Link = { lost: 0, ended: false };

const RETRY_FIRST_MS = 1000;
const RETRY_MAX_MS = 15000;

/// What a rejected `agent_view` said; anything else is a failure to show.
export function viewError(e: unknown): ViewError {
  const kind = typeof e === "object" && e ? (e as { kind?: unknown }).kind : undefined;
  if (kind === "lost" || kind === "gone") return { kind };
  if (kind === "failed") return { kind, message: String((e as { message?: unknown }).message ?? "") };
  return { kind: "failed", message: String(e) };
}

/// The wait before reading again after `lost` failed reads: 1 s, doubling to 15 s.
export const retryDelay = (lost: number) => Math.min(RETRY_FIRST_MS * 2 ** Math.max(0, lost - 1), RETRY_MAX_MS);

export const lostNote = (machine: string) => `Lost the connection to ${machine}. Reconnecting...`;

/// Why `agent_steer` failed: the machine could not be reached and nothing
/// went ("lost"), the link dropped once it went ("unsure"), or a refusal.
export interface SendError { kind: "lost" | "unsure" | "failed"; message: string }

export function sendError(e: unknown): SendError {
  const o = typeof e === "object" && e ? e as { kind?: unknown; message?: unknown } : null;
  const kind = o?.kind === "lost" || o?.kind === "unsure" ? o.kind : "failed";
  return { kind, message: String(o ? o.message ?? "" : e).replace(/^bad request: /, "") };
}

/// How long a message waits for its machine to come back before it fails.
export const HOLD_MS = 120_000;
export const holdNote = (machine: string) => `Waiting for ${machine} to come back`;

/// Whether `page`, read after a send that may have gone, shows the node took
/// `text`: it holds more messages than the `queued` before, or the user's
/// last message in the log is it.
export function tookIt(page: Pick<AgentPage, "queued" | "entries">, queued: number, text: string) {
  if (page.queued > queued) return true;
  const last = page.entries.findLast(e => e.kind === "user");
  return !!text.trim() && !!last && last.text.includes(text.trim());
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
