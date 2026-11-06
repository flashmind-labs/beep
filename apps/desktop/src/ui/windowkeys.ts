// ⌘ keys the command center window keeps for itself while its pane (the
// bar's views, inside the window) has focus: the palette, Settings and the
// window's own pages.

const WINDOW_KEYS = new Set(["k", ",", "/", "m", "j", "l", "r"]);

/// Whether the window takes ⌘`key` while its pane shows `paneView`, rather
/// than the pane: not ⌘M on the start form, which picks the model there,
/// nor ⌘R in a list of chats, which renames one.
export function windowTakes(key: string, paneView?: string | null) {
  const k = key.toLowerCase();
  if (k === "m" && paneView === "agentnew") return false;
  if (k === "r" && paneView === "chats") return false;
  return WINDOW_KEYS.has(k);
}
