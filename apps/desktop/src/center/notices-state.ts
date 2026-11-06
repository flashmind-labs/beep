// The bell's logic, with no page behind it: what the assistant told the user
// on its own (alerts, from a server), what the mesh told this Mac (notes,
// kept here), which are read, and what the bell shows. With no server the
// alerts are `null` and the notes alone fill the pane.
import type { PaneTarget } from "../bar/pane.ts";

/// What the assistant told the user, as `alerts_list` gives it.
export interface Alert {
  session_id: string;
  title: string;
  source: string;
  text: string;
  read: boolean;
  /// Milliseconds since the Unix epoch.
  at: number;
}

/// `null` when there are no alerts to have.
export type Alerts = Alert[] | null;

export type NoteKind = "finished" | "asks" | "conflict" | "move_failed" | "offline";

/// What the mesh told this Mac, as `notes_list` gives it.
export interface Note {
  id: string;
  kind: NoteKind;
  title: string;
  text: string;
  source: string;
  at: number;
  read: boolean;
  /// Answered, settled or back online.
  done: boolean;
  /// The session a click opens.
  target: PaneTarget;
}

/// One row of the pane.
export interface Item {
  id: string;
  /// An alert from the server, read and dismissed there.
  server: boolean;
  kind: NoteKind | "alert";
  title: string;
  text: string;
  source: string;
  at: number;
  read: boolean;
  done: boolean;
  target: PaneTarget;
}

export const fromAlert = (a: Alert): Item => ({ id: a.session_id, server: true, kind: "alert", title: a.title, text: a.text, source: a.source, at: a.at, read: a.read, done: false, target: null });
export const fromNote = (n: Note): Item => ({ ...n, server: false, target: n.target ?? null });

/// The server's alerts, none with no server, and the mesh's notes, newest first.
export function merge(alerts: Item[] | null, notes: Item[]): Item[] {
  return [...(alerts ?? []), ...notes].sort((a, b) => b.at - a.at);
}

/// The most the badge counts to.
const BADGE_MAX = 9;

/// How many rows are not read yet.
export function unreadCount(items: Item[]): number {
  return items.filter(i => !i.read).length;
}

/// The badge's text, empty when everything is read.
export function badge(items: Item[]): string {
  const n = unreadCount(items);
  if (!n) return "";
  return n > BADGE_MAX ? `${BADGE_MAX}+` : String(n);
}

/// The bell's spoken name.
export function bellLabel(items: Item[]): string {
  const n = unreadCount(items);
  return n ? `Notifications, ${n} unread` : "Notifications";
}

/// The ids of the rows not read yet.
export function unreadIds(items: Item[]): string[] {
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
export function find(items: Item[], id: string | null): Item | null {
  return (id && items.find(i => i.id === id)) || null;
}

/// `ids` split by where they are read and dismissed.
export function bySide(items: Item[], ids: string[]): { server: string[]; mesh: string[] } {
  const server = new Set(items.filter(i => i.server).map(i => i.id));
  return { server: ids.filter(id => server.has(id)), mesh: ids.filter(id => !server.has(id)) };
}

/// What a row says once it is over: an ask answered, conflicts settled, a
/// machine back.
export function doneLabel(i: Item): string {
  if (!i.done) return "";
  return i.kind === "asks" ? "Answered" : i.kind === "conflict" ? "Settled" : i.kind === "offline" ? "Back online" : "";
}

/// Whether the row still asks, with Allow and Deny.
export const asks = (i: Item) => i.kind === "asks" && !i.done;

/// What a click on a row does: an alert opens in the pane, the rest open
/// their session, the Sync page or Machines.
export function opens(i: Item): "alert" | "session" | "sync" | "machines" | null {
  if (i.server) return "alert";
  if (i.target) return "session";
  if (i.kind === "conflict") return "sync";
  if (i.kind === "offline") return "machines";
  return null;
}

/// The sub line of a row: where it came from and when.
export function subLine(i: Item, ago: string): string {
  return `${i.source} · ${ago}`;
}
