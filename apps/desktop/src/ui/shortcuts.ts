// Every keyboard shortcut of the app, by where it works. The Keyboard shortcuts
// sheet (⌘/ and Settings) draws this list and nothing else does.

/// What a row needs to be shown: the Beep server, or live mode. Rows whose
/// feature is off are left out.
export type Needs = "server" | "live";

/// One shortcut: its key caps ("/" between alternatives, a space between keys
/// pressed in turn) and what it does.
export interface Shortcut {
  keys: string;
  does: string;
  needs?: Needs;
}

export interface ShortcutSection {
  title: string;
  rows: readonly Shortcut[];
}

/// What is on: the server rows show when `server`, the live rows when `live`.
export interface Features {
  server: boolean;
  live: boolean;
}

export const SHORTCUTS: readonly ShortcutSection[] = [
  { title: "Anywhere", rows: [
    { keys: "⌥ ⌥", does: "Open or hide Beep" },
    { keys: "tap ⌥", does: "Start live mode, in the open bar", needs: "live" },
    { keys: "hold ⌥", does: "Talk while held", needs: "live" },
  ] },
  { title: "Window", rows: [
    { keys: "⌘K", does: "Search" },
    { keys: "⌘,", does: "Settings" },
    { keys: "⌘/", does: "Keyboard shortcuts" },
    { keys: "⌘N", does: "New agent session" },
    { keys: "⌘[", does: "Back" },
    { keys: "⌘]", does: "Forward" },
    { keys: "⌘W", does: "Close tab, else hide" },
    { keys: "esc", does: "Up one page" },
    { keys: "⌘M", does: "Memories", needs: "server" },
    { keys: "⌘J", does: "Routines", needs: "server" },
    { keys: "⌘L", does: "Skills", needs: "server" },
    { keys: "⌘R", does: "Research", needs: "server" },
  ] },
  { title: "Projects", rows: [
    { keys: "⇧⌘N", does: "Open project in a window" },
    { keys: "⌘T", does: "New terminal in the project" },
    { keys: "↩", does: "Start a session from the project input" },
  ] },
  { title: "Agent session", rows: [
    { keys: "↩", does: "Send, or steer a running turn" },
    { keys: "⌥↩", does: "Queue for after the turn" },
    { keys: "⇧↩", does: "New line" },
    { keys: "⇧tab", does: "Change mode" },
    { keys: "⌘↩", does: "Allow, or start the agent" },
    { keys: "⌘D", does: "Deny" },
    { keys: "⌘.", does: "Stop the turn" },
    { keys: "⌘M", does: "Pick the model, on the start form" },
    { keys: "⌘U", does: "Open or fold thoughts" },
    { keys: "⌘Y", does: "Open or fold commands" },
    { keys: "⇧⌘Y", does: "Open or fold the last command" },
    { keys: "home / ⌘↑", does: "Top of the log" },
    { keys: "end / ⌘↓", does: "End of the log" },
  ] },
  { title: "Side pane", rows: [
    { keys: "⌘1", does: "Files" },
    { keys: "⌘2", does: "Changes" },
    { keys: "⌘3", does: "Artifacts" },
    { keys: "⌘O", does: "Open folder in the editor" },
    { keys: "⇧esc", does: "Zoom the pane" },
    { keys: "⌃tab", does: "Next tab" },
    { keys: "⌃⇧tab", does: "Previous tab" },
    { keys: "esc", does: "Terminal back to the input" },
  ] },
  { title: "Terminals", rows: [
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
    { keys: "⌘↩", does: "Ask Beep", needs: "server" },
    { keys: "esc", does: "Close" },
  ] },
  { title: "Lists", rows: [
    { keys: "↑ ↓", does: "Move", needs: "server" },
    { keys: "tab", does: "Actions", needs: "server" },
    { keys: "⌘R", does: "Rename", needs: "server" },
    { keys: "⌘P", does: "Pin", needs: "server" },
    { keys: "⌘⌫", does: "Delete or forget", needs: "server" },
  ] },
  { title: "Sheets and menus", rows: [
    { keys: "esc", does: "Close" },
    { keys: "↑ ↓", does: "Move in a menu" },
    { keys: "⌘↩", does: "Confirm a sheet" },
  ] },
];

/// The sections whose features are on; a section left with no rows goes.
export function visibleShortcuts(on: Features, sections: readonly ShortcutSection[] = SHORTCUTS): ShortcutSection[] {
  return sections
    .map(s => ({ title: s.title, rows: s.rows.filter(r => !r.needs || on[r.needs]) }))
    .filter(s => s.rows.length);
}

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
