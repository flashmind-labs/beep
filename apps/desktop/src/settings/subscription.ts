// What a Mac with no subscription shows. Machines, projects, terminals and coding agents work
// over the mesh; the assistant needs the subscription, so what is for it is left out.

import type { ViewName } from "../center/state.ts";

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

/// The shortcut sheet's sections for a bar that is `linked` or not. Without a subscription there are
/// no chats, live mode or assistant pages, so only the general keys and lists stay, and ⌘N starts
/// an agent.
export function shortcutSheet(sections: [string, readonly (readonly string[])[]][], linked: boolean): [string, readonly (readonly string[])[]][] {
  if (linked) return sections;
  return sections
    .filter(([title]) => title === "Anywhere" || title === "In a list" || title === "In an agent or research")
    .map(([title, rows]): [string, readonly (readonly string[])[]] => [title, title !== "Anywhere" ? rows : rows
      .filter(([k]) => !(k.length === 2 && k[0] === "⌘" && keyHidden(k[1].toLowerCase(), linked)))
      .map(([k, t]) => (k === "⌘N" ? [k, "New agent"] : [k, t]))]);
}

/// Pane pages that are the assistant's.
const ASSISTANT_PAGES = ["chats", "memories", "routines", "rnew", "grants", "skills", "apps", "mcp", "appperms", "research", "model", "group", "ginfo", "gnew", "invite"];

/// Pane pages that are the server's own: linking, signing in and the removed Mac's page.
const SERVER_PAGES = ["link", "invite", "revoked"];

/// Whether going to window view `name` with `arg` opens something only a subscription has, such as
/// a deep link from the tray, another window or a notification, or with no `server` at all one of
/// its own pages. The window shows Home instead.
export function viewHidden(name: ViewName, arg: unknown, subscribed: boolean, server: boolean, local = false) {
  if (!server && name === "pane" && typeof arg === "string" && SERVER_PAGES.includes(arg)) return true;
  if (subscribed) return false;
  // Chats kept on this Mac: the chat list, a chat and its model.
  if (local && name === "pane" && (arg === "chats" || arg === "model" || (typeof arg === "object" && arg !== null && "chat" in arg))) return false;
  if (name === "research" || name === "researchopen") return true;
  if (name !== "pane") return false;
  if (typeof arg === "string") return ASSISTANT_PAGES.includes(arg);
  return typeof arg === "object" && arg !== null && ("chat" in arg || "group" in arg);
}

/// Whether the first-run window shows: only with no subscription and before it was answered. A
/// Mac signed out has an account, so it never asks.
export function firstRunDue(subscribed: boolean, kept: string | null, signedOut = false) {
  return !subscribed && !signedOut && kept === null;
}
