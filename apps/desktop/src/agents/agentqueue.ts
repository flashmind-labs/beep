// Messages typed while a coding agent's turn runs.

/// An image waiting to go with a message.
export interface Attached {
  name: string;
  mime: string;
  size: number;
  /// Base64, and as a data URL for its chip.
  data: string;
  url: string;
}

/// Where a steer is: on its way to the node, held there for the next tool
/// boundary, or delivered to the agent.
export type SteerState = "sending" | "sent" | "delivered";

/// A message typed while the turn runs.
export interface Waiting {
  how: "steer" | "queue";
  /// What was typed, as the row shows it.
  typed: string;
  /// What goes to the agent: the typed text and an `@path` per mention.
  text: string;
  paths: string[];
  images: Attached[];
  /// A steer's progress; null for a queued message.
  state: SteerState | null;
  /// For a sent steer: the session read after which the node holds it.
  after: number;
}

/// One message of queued ones: their words and images, in order.
export interface Batch {
  text: string;
  images: Attached[];
}

/// The queued messages as few messages as fit: in order, joined until the
/// images would pass what one message takes.
export function batches(items: Waiting[], maxImages: number, maxBytes: number): Batch[] {
  const out: Batch[] = [];
  const bytes = (b: Batch) => b.images.reduce((n, a) => n + a.size, 0);
  for (const item of items) {
    const last = out[out.length - 1];
    const fits = last && last.images.length + item.images.length <= maxImages && bytes(last) + bytes(item) <= maxBytes;
    if (fits) {
      last.text = [last.text, item.text].filter(Boolean).join("\n\n");
      last.images = [...last.images, ...item.images];
    } else {
      out.push({ text: item.text, images: [...item.images] });
    }
  }
  return out;
}

/// Marks the steers the node no longer holds as delivered, oldest first.
export function markDelivered(items: Waiting[], waiting: number, read: number) {
  const held = items.filter(q => q.how === "steer" && q.state === "sent" && q.after < read);
  const gone = Math.max(0, held.length - waiting);
  for (const q of held.slice(0, gone)) q.state = "delivered";
  return gone > 0;
}

/// Drops delivered steers before drawing the queue.
export function dropDelivered(items: Waiting[]) {
  items.splice(0, items.length, ...items.filter(q => q.state !== "delivered"));
}

/// A stop ends when its turn finishes, even if the next turn starts before a read.
export function stopPending(stop: { id: string; turns: number } | null, id: string, active: boolean, turns: number) {
  return stop?.id === id && active && turns <= stop.turns ? stop : null;
}
