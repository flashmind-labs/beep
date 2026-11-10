// Messages typed while a coding agent's turn runs.
import type { StashFile } from "../chat/stash-state.ts";
import type { Run } from "./older-state.ts";

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
type SteerState = "sending" | "sent" | "delivered";

/// A message typed while the turn runs.
export interface Waiting {
  how: "steer";
  /// What was typed, as the row shows it.
  typed: string;
  /// What goes to the agent: the typed text and an `@path` per mention.
  text: string;
  paths: string[];
  images: Attached[];
  files?: { path: string; file: StashFile }[];
  /// A steer's progress; null before sending.
  state: SteerState | null;
  /// For a sent steer: the session read after which the node holds it.
  after: number;
}

/// Marks the steers the node no longer holds as delivered, oldest first.
export function markDelivered(items: Waiting[], waiting: number, read: number) {
  const held = items.filter(q => q.how === "steer" && q.state === "sent" && q.after < read);
  const gone = Math.max(0, held.length - waiting);
  for (const q of held.slice(0, gone)) q.state = "delivered";
  return gone > 0;
}

/// Rebuild the pending rows from the node after navigation or reconnecting.
/// Match each occurrence once, keeping local attachments and sends in flight.
export function syncSteers(items: Waiting[], run: Run & { queued: number; queued_messages?: string[] }, read: number) {
  const messages = run.queued_messages;

  if (!messages || messages.length !== run.queued) {
    markDelivered(items, run.queued, read);
    return;
  }

  const local = items.filter(q => q.state !== "delivered").reverse();
  const matched = new Set<Waiting>();
  const restored: Waiting[] = [];

  for (const text of [...messages].reverse()) {
    const q = local.find(q => !matched.has(q)
      && [q.text, ...q.images.map(image => `[Image: ${image.name}]`)].filter(Boolean).join("\n") === text);
    if (q) matched.add(q);
    else restored.unshift({
      how: "steer", typed: text, text, paths: [], images: [], state: "sent", after: read,
    });
  }

  for (const q of local) {
    if (q.state === "sent" && q.after < read && !matched.has(q)) q.state = "delivered";
  }

  items.push(...restored);
}

/// Drops delivered steers before drawing the queue.
export function dropDelivered(items: Waiting[]) {
  items.splice(0, items.length, ...items.filter(q => q.state !== "delivered"));
}

/// A stop ends when its turn finishes, even if the next turn starts before a read.
export function stopPending(stop: { id: string; turns: number } | null, id: string, active: boolean, turns: number) {
  return stop?.id === id && active && turns <= stop.turns ? stop : null;
}
