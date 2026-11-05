// The bell's logic, with no page behind it: what the assistant told the user
// on its own (alerts), which are read, and what the bell shows. A server
// from before alerts, no server, or an app not linked has none: the list is
// `null`, the bell is hidden and chats are as they always were.

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

/// The most the badge counts to.
const BADGE_MAX = 9;

/// How many alerts are not read yet.
export function unreadCount(alerts: Alerts): number {
  return alerts ? alerts.filter(a => !a.read).length : 0;
}

/// The bell shows only where alerts exist.
export function bellShown(alerts: Alerts): boolean {
  return alerts !== null;
}

/// The badge's text, empty when everything is read.
export function badge(alerts: Alerts): string {
  const n = unreadCount(alerts);
  if (!n) return "";
  return n > BADGE_MAX ? `${BADGE_MAX}+` : String(n);
}

/// The bell's spoken name.
export function bellLabel(alerts: Alerts): string {
  const n = unreadCount(alerts);
  return n ? `Notifications, ${n} unread` : "Notifications";
}

/// `alerts` with those in `ids` read.
export function markRead(alerts: Alerts, ids: string[]): Alerts {
  return alerts && alerts.map(a => (ids.includes(a.session_id) ? { ...a, read: true } : a));
}

/// `alerts` without those in `ids`, dismissed.
export function without(alerts: Alerts, ids: string[]): Alerts {
  return alerts && alerts.filter(a => !ids.includes(a.session_id));
}

/// The alert `id` names, if the list still has it.
export function find(alerts: Alerts, id: string | null): Alert | null {
  return (id && alerts?.find(a => a.session_id === id)) || null;
}

/// The ids of every alert, for Clear.
export function ids(alerts: Alerts): string[] {
  return alerts ? alerts.map(a => a.session_id) : [];
}

/// The sub line of a row: where it came from and when.
export function subLine(a: Alert, ago: string): string {
  return `${a.source} · ${ago}`;
}
