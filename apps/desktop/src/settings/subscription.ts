// What a Mac with no subscription shows. Machines, projects, terminals and coding agents work
// over the mesh; the assistant needs the subscription, so what is for it is left out.

/// Why to subscribe, as Settings says it.
export const SUBSCRIPTION_NOTE = "Adds chats, memories, routines, skills, apps, research, the project agent and phones.";

/// Sidebar groups that are the assistant's.
const ASSISTANT_GROUPS = ["chats", "research"];

/// Whether sidebar group `name` shows.
export function groupShown(name: string, subscribed: boolean) {
  return subscribed || !ASSISTANT_GROUPS.includes(name);
}

/// The window's own pages the Beep group lists: all of them with a subscription, else Settings.
export function beepViews<T extends { view: string }>(views: readonly T[], subscribed: boolean) {
  return subscribed ? [...views] : views.filter(v => v.view === "settings");
}

/// How signing in goes: in the browser on a hosted server, else the account starts on this Mac.
export function signInWay(hosted: boolean) {
  return hosted ? "browser" : "here";
}

/// ⌘ and a letter that open the assistant's pages: memories, routines, skills, research, groups
/// and the chat list.
const ASSISTANT_KEYS = ["m", "j", "l", "r", "g", "k"];

/// Whether ⌘ and `key` does nothing without a subscription.
export function keyHidden(key: string, subscribed: boolean) {
  return !subscribed && ASSISTANT_KEYS.includes(key);
}

/// Pane pages that are the assistant's.
const ASSISTANT_PAGES = ["chats", "memories", "routines", "rnew", "grants", "skills", "apps", "mcp", "appperms", "research", "model", "group", "ginfo", "gnew", "invite"];

/// Whether going to window view `name` with `arg` opens something only a subscription has, such as
/// a deep link from the tray, another window or a notification. The window shows Home instead.
export function viewHidden(name: string, arg: unknown, subscribed: boolean) {
  if (subscribed) return false;
  if (name === "research") return true;
  if (name !== "pane") return false;
  if (typeof arg === "string") return ASSISTANT_PAGES.includes(arg);
  return typeof arg === "object" && arg !== null && ("chat" in arg || "group" in arg);
}

/// Whether the first-run window shows: only with no subscription and before it was answered.
export function firstRunDue(subscribed: boolean, kept: string | null) {
  return !subscribed && kept === null;
}
