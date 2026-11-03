// Messages typed while a coding agent's turn runs. A steer goes to the node
// at once, which delivers it once the agent's current tool call finishes; a
// queued message stays here and goes once the turn stops or ends. Each
// carries what was attached to it: images, and the paths it mentions.

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
/// boundary, or delivered to the agent (shown for a moment, then dropped).
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
  /// When a steer was seen delivered, to drop its row a moment later.
  at?: number;
}

/// How long a delivered steer's row stays.
export const DELIVERED_SHOWN_MS = 4000;

/// One message of queued ones: their words and images, in order.
export interface Batch {
  text: string;
  images: Attached[];
}

/// The queued messages as few messages as fit: in order, joined until the
/// images would pass what one message takes.
export function batches(items: Waiting[], maxImages: number, maxBytes: number): Batch[] {
  const out: Batch[] = [];
  for (const item of items) {
    const last = out[out.length - 1];
    const bytes = (b: Batch) => b.images.reduce((n, a) => n + a.size, 0);
    const fits = last && last.images.length + item.images.length <= maxImages
      && bytes(last) + item.images.reduce((n, a) => n + a.size, 0) <= maxBytes;
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
/// `waiting` is what the node said waits (its steers and prompts) in read
/// number `read`; only steers it had before that read are judged, so a
/// read that left before a steer arrived never counts it delivered. True
/// when one changed.
export function markDelivered(items: Waiting[], waiting: number, read: number, now = Date.now()) {
  const held = items.filter(q => q.how === "steer" && q.state === "sent" && q.after < read);
  const gone = Math.max(0, held.length - waiting);
  for (const q of held.slice(0, gone)) { q.state = "delivered"; q.at = now; }
  return gone > 0;
}

/// Drops the delivered steers shown long enough. True when one went.
export function dropDelivered(items: Waiting[], now = Date.now()) {
  const before = items.length;
  const kept = items.filter(q => q.state !== "delivered" || now - (q.at ?? now) < DELIVERED_SHOWN_MS);
  items.splice(0, items.length, ...kept);
  return kept.length !== before;
}

/// Esc: the steers not delivered yet go once the stop lands, with the
/// queue, since stopping drops what the node holds.
export function steersToQueue(items: Waiting[]) {
  for (const q of items) {
    if (q.how === "steer" && q.state !== "delivered") { q.how = "queue"; q.state = null; }
  }
}
