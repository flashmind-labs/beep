// The bar's prompt: how tall it grows, and the height its grip sets.

const ASK_HEIGHT = "bar.askHeight", ASK_MAX = 420;
/// A height the user dragged the bar's prompt to; null lets it follow its lines.
let askHeight: number | null = null;
try { askHeight = Number(localStorage.getItem(ASK_HEIGHT)) || null; } catch {}

/// The prompt grows with its lines, up to six, then scrolls. A height the
/// user dragged it to is the least it shows, so its lines still fit.
export function grow(ask: HTMLTextAreaElement) {
  ask.style.height = "auto";
  const lines = Math.min(ask.scrollHeight, 156);
  const dragged = askHeight && ask.id === "ask" ? Math.min(askHeight, ASK_MAX) : 0;
  ask.style.height = Math.max(lines, dragged) + "px";
}

/// The grip was dragged to make the prompt `height` tall.
export function dragAsk(ask: HTMLTextAreaElement, height: number) {
  askHeight = Math.max(26, Math.min(height, ASK_MAX));
  grow(ask);
}

/// The drag ended: the height is kept for next time.
export function keepAskHeight() {
  try { if (askHeight) localStorage.setItem(ASK_HEIGHT, String(askHeight)); } catch {}
}

/// The prompt follows its lines again.
export function resetAskHeight(ask: HTMLTextAreaElement) {
  askHeight = null;
  try { localStorage.removeItem(ASK_HEIGHT); } catch {}
  grow(ask);
}
