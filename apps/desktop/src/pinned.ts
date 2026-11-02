// Splits the chat list for the sidebar: pinned chats and groups go to the
// Pinned group, the newest of the rest stay under Chats.

/// `rows` are newest first, as the pane lists them.
export function splitPinned(rows, limit) {
  return {
    pinned: rows.filter(c => c.pinned),
    recent: rows.filter(c => !c.pinned).slice(0, limit),
  };
}
