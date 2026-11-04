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
