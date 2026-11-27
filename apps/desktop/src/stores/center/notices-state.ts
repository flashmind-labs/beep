// The bell's logic, with no page behind it: what the mesh told this Mac
// (notes, kept here), which are read, and what the bell shows.
import type { PaneTarget } from "../../shell/bar/pane.ts";
import type { ProjectsArg } from "./state.ts";
import { bytes, formatAgo } from "../../ui/format.ts";

export type NoteKind = "finished" | "asks" | "conflict" | "move_failed" | "offline" | "skill_conflict" | "run" | "removed" | "review_requested" | "assigned";

/// A build's note opens its project on the run; a review request or assignment opens its page.
type NoteTarget = PaneTarget | { projects: ProjectsArg } | { link: string };

/// What the mesh told this Mac, as `notes_list` gives it.
export interface Note {
  id: string;
  kind: NoteKind;
  title: string;
  text: string;
  /// `text` as cleaned inline HTML, for a coding agent's reply in markdown.
  html: string;
  source: string;
  at: number;
  read: boolean;
  /// Answered, settled or back online.
  done: boolean;
  /// The session, or a build's project, a click opens.
  target: NoteTarget;
  /// The lines of `text` that are on this Mac, for a skill conflict.
  here?: string[];
  /// Each copy of a skill conflict, to keep one.
  held?: Held[];
  /// Allow and Deny answer its question; any other question opens its chat.
  allow_deny: boolean;
}

/// One copy of a conflicting skill as the node told it.
export interface Held {
  /// The machine's endpoint id.
  machine: string;
  /// The machine's name.
  label: string;
  folder: string;
  hash: string;
  /// When a file of it last changed, milliseconds since the epoch; 0 unknown.
  changed: number;
  bytes: number;
}

/// A note as a row: one from before may lack a target.
export const fromNote = (n: Note): Note => ({ ...n, target: n.target ?? null });

/// The most the badge counts to.
const BADGE_MAX = 9;

/// How many rows are not read yet.
export function unreadCount(items: Note[]): number {
  return items.filter(i => !i.read).length;
}

/// The badge's text, empty when everything is read.
export function badge(items: Note[]): string {
  const n = unreadCount(items);
  if (!n) return "";
  return n > BADGE_MAX ? `${BADGE_MAX}+` : String(n);
}

/// The bell's spoken name.
export function bellLabel(items: Note[]): string {
  const n = unreadCount(items);
  return n ? `Notifications, ${n} unread` : "Notifications";
}

/// The ids of the rows not read yet.
export function unreadIds(items: Note[]): string[] {
  return items.filter(i => !i.read).map(i => i.id);
}

/// `list` with those in `ids` read.
export function markRead<T extends { id: string; read: boolean }>(list: T[], ids: string[]): T[] {
  return list.map(i => (ids.includes(i.id) ? { ...i, read: true } : i));
}

/// `list` without those in `ids`, dismissed.
export function without<T extends { id: string }>(list: T[], ids: string[]): T[] {
  return list.filter(i => !ids.includes(i.id));
}

/// The row `id` names, if the list still has it.
export function find(items: Note[], id: string | null): Note | null {
  return (id && items.find(i => i.id === id)) || null;
}

/// What a row says once it is over: an ask answered, conflicts settled, a
/// machine back.
export function doneLabel(i: Note): string {
  if (!i.done) return "";
  return i.kind === "asks" ? "Answered" : i.kind === "conflict" || i.kind === "skill_conflict" ? "Settled" : i.kind === "offline" ? "Back online" : i.kind === "review_requested" ? "Reviewed" : i.kind === "assigned" ? "Unassigned" : "";
}

/// Whether the row still asks, with Allow and Deny.
export const asks = (i: Note) => i.kind === "asks" && !i.done && i.allow_deny;

/// One copy of a skill: its machine and folder, and whether it is on this Mac.
export interface Copy {
  machine: string;
  folder: string;
  here: boolean;
  /// What the node told of it, to keep it; none in a note from before.
  held?: Held;
  /// It changed last of the copies.
  newest?: boolean;
}

/// A skill conflict's copies: as the node told them, or one per line of its
/// text, `machine folder`, in a note from before.
export function copies(i: Note): Copy[] {
  if (i.held?.length) {
    const last = Math.max(...i.held.map(c => c.changed));
    return i.held.map(c => ({ machine: c.label, folder: c.folder, here: !!i.here?.includes(`${c.label} ${c.folder}`), held: c, newest: last > 0 && c.changed === last }));
  }

  return i.text.split("\n").filter(Boolean).map(line => {
    const at = line.indexOf(" ~/");
    return { machine: at < 0 ? line : line.slice(0, at), folder: at < 0 ? "" : line.slice(at + 1), here: !!i.here?.includes(line) };
  });
}

/// The small line under a copy: `Changed 3 h ago · 2.1 KB · newest`.
export function copyLine(c: Copy, now = Date.now()): string {
  if (!c.held) return "";
  const parts = [c.held.changed ? `Changed ${formatAgo(c.held.changed, "since", now)}` : "", bytes(c.held.bytes), c.newest ? "newest" : ""];
  return parts.filter(Boolean).join(" · ");
}

/// The skill's name in a conflict's title, `Skill x differs on a and b`.
export function skillName(i: Note): string {
  return /^Skill (.+) differs on /.exec(i.title)?.[1] ?? "";
}

/// The project a sync conflict is in, from its id `conflict:<project>:<machine>`.
export function conflictProject(i: Note): string | null {
  return /^conflict:(.+):[^:]+$/.exec(i.id)?.[1] ?? null;
}

/// What a click on a row does: a skill conflict opens in the pane, the rest
/// open their session, the project a sync conflict is in, Machines, or a pull request or issue in the browser.
export function opens(i: Note): "skill" | "session" | "project" | "conflict" | "machines" | "link" | null {
  if (i.kind === "skill_conflict") return "skill";
  if (i.target && typeof i.target === "object" && "link" in i.target) return "link";
  if (i.target && typeof i.target === "object" && "projects" in i.target) return "project";
  if (i.target) return "session";
  if (i.kind === "conflict") return "conflict";
  if (i.kind === "offline" || i.kind === "removed") return "machines";
  return null;
}

/// The sub line of a row: where it came from and when. A coding agent's
/// note says only when, as its title names the project, but for a thread's,
/// which names its `chat`.
export function subLine(i: Note, ago: string, chat: string | null = null): string {
  if (chat) return `${chat} · ${ago}`;
  return noteAgent(i) ? ago : `${i.source} · ${ago}`;
}

/// The coding agent a note is about, such as `claude`.
export function noteAgent(i: Note): string | null {
  return i.target && typeof i.target === "object" && "agent" in i.target ? i.target.agent.agent : null;
}
