// Every keyboard shortcut of the app, by where it works. The Keyboard shortcuts
// sheet (⌘/ and Settings) draws this list and nothing else does.

/// One shortcut: its key caps ("/" between alternatives, a space between keys
/// pressed in turn) and what it does.
interface Shortcut {
  keys: string;
  does: string;
}

export interface ShortcutSection {
  title: string;
  rows: readonly Shortcut[];
}

export const SHORTCUTS: readonly ShortcutSection[] = [
  { title: "Anywhere", rows: [
    { keys: "⌥ ⌥", does: "Open or hide Beep" },
  ] },
  { title: "General", rows: [
    { keys: "⌘= ⌘-", does: "Zoom in, out" },
    { keys: "⌘0", does: "Actual size" },
  ] },
  { title: "Window", rows: [
    { keys: "⌘K", does: "Search" },
    { keys: "⌘B", does: "Notifications" },
    { keys: "⌥Tab", does: "Switch active conversations; hold Option to choose" },
    { keys: "⌥⇧Tab", does: "Previous active conversation" },
    { keys: "⌃Tab", does: "Next running session in this project or chat" },
    { keys: "⌃⇧Tab", does: "Previous running session in this project or chat" },
    { keys: "⌘E", does: "Fold the sidebar; beside a project's sessions or a chat's threads, show it over them" },
    { keys: "⌘,", does: "Settings" },
    { keys: "⌘/", does: "Keyboard shortcuts" },
    { keys: "⌘N", does: "New agent session; on the Assistant page, a new chat; beside a chat's threads, a new thread" },
    { keys: "⇧⌘N", does: "Open the chat in a window" },
    { keys: "⌘L", does: "Assistant" },
    { keys: "⌘[", does: "Back" },
    { keys: "⌘]", does: "Forward" },
    { keys: "⌘W", does: "Close tab, else hide" },
    { keys: "esc", does: "Up one page; in a chat with threads, all chats first, then the sidebar" },
  ] },
  { title: "Projects", rows: [
    { keys: "⇧⌘N", does: "Open project in a window" },
    { keys: "⌘F", does: "Search projects, on the Projects page" },
    { keys: "⌘T", does: "New terminal in the project" },
    { keys: "↩", does: "Start a session from the project input" },
  ] },
  { title: "Find in chat", rows: [
    { keys: "⌘F", does: "Find in the conversation" },
    { keys: "⌘G / ⇧⌘G", does: "Next or previous match, while searching" },
    { keys: "↩", does: "Next match, in the search field" },
    { keys: "⇧↩", does: "Previous match, in the search field" },
    { keys: "esc", does: "Close search" },
  ] },
  { title: "Agent session", rows: [
    { keys: "↩", does: "Send, or steer a running turn" },
    { keys: "⇧↩", does: "New line" },
    { keys: "hold ⌥", does: "Dictate while held" },
    { keys: "⇧tab", does: "Change mode" },
    { keys: "⌘↩", does: "Allow, or start the agent" },
    { keys: "⌘D", does: "Deny" },
    { keys: "⌘.", does: "Stop the turn" },
    { keys: "⇧⌘W", does: "Close session" },
    { keys: "⇧⌘M", does: "Pick the model, on the start form and in a conversation" },
    { keys: "↑ ↓", does: "Move through models, in the model search" },
    { keys: "⌘U", does: "Open or fold thoughts" },
    { keys: "⌘Y", does: "Open or fold commands" },
    { keys: "⇧⌘Y", does: "Open or fold the last command" },
    { keys: "home / ⌘↑", does: "Top of the log" },
    { keys: "end / ⌘↓", does: "End of the log" },
  ] },
  { title: "Side pane", rows: [
    { keys: "⌘1", does: "Focus chat" },
    { keys: "⌘2-9", does: "Pane tab by its place" },
    { keys: "⌘O", does: "Open folder in the editor, or from the bar the session in the window" },
    { keys: "⇧esc", does: "Zoom the pane" },
    { keys: "⌃tab", does: "Next tab" },
    { keys: "⌃⇧tab", does: "Previous tab" },
    { keys: "esc", does: "Terminal back to the input" },
  ] },
  { title: "Terminals", rows: [
    { keys: "Shift+Esc", does: "Focus the selected terminal on the Terminals page" },
    { keys: "⌘N", does: "New terminal like this one" },
    { keys: "⌃`", does: "New terminal in the session folder" },
    { keys: "⌃← / ⌥←", does: "Word back" },
    { keys: "⌃→ / ⌥→", does: "Word forward" },
    { keys: "⌘←", does: "Line start" },
    { keys: "⌘→", does: "Line end" },
    { keys: "⌥⌫", does: "Delete word" },
    { keys: "⌘⌫", does: "Delete to line start" },
    { keys: "⌥⌦", does: "Delete next word" },
  ] },
  { title: "Review", rows: [
    { keys: "j", does: "Next file" },
    { keys: "k", does: "Previous file" },
    { keys: "x", does: "Fold the file" },
    { keys: "/", does: "Filter files" },
    { keys: "⌘↩", does: "Add the comment" },
  ] },
  { title: "Search", rows: [
    { keys: "↑ ↓", does: "Move" },
    { keys: "↩", does: "Open" },
    { keys: "tab", does: "Back to the input" },
    { keys: "esc", does: "Close" },
  ] },
  { title: "Sheets and menus", rows: [
    { keys: "esc", does: "Close" },
    { keys: "↑ ↓", does: "Move in a menu" },
    { keys: "⌘↩", does: "Confirm a sheet" },
  ] },
];

/// Case and spaces ignored: "⌘k" finds ⌘K.
const squash = (s: string) => s.toLowerCase().replace(/\s+/g, "");

/// The rows that match what was typed in the filter, in their sections: by a word of
/// what they do, by their keys, or all of a section whose title matches.
export function filterShortcuts(sections: readonly ShortcutSection[], query: string): ShortcutSection[] {
  const q = squash(query);
  if (!q) return [...sections];
  const out: ShortcutSection[] = [];

  for (const s of sections) {
    if (squash(s.title).includes(q)) { out.push(s); continue; }
    const rows = s.rows.filter(r => squash(r.does).includes(q) || squash(r.keys).includes(q));
    if (rows.length) out.push({ title: s.title, rows });
  }

  return out;
}
