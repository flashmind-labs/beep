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
  /// The original uploaded picture: its path on the session's
  /// machine, or the file waiting for a session to start.
  original: string | File | null;
}

/// Where a message is: on its way to the node, held there for the next tool
/// boundary, or delivered to the agent.
export type SteerState = "sending" | "sent" | "delivered";

/// A message typed while the turn runs.
export interface Waiting {
  /// What was typed, as the row shows it.
  typed: string;
  /// What goes to the agent: the typed text and an `@path` per mention.
  text: string;
  paths: string[];
  images: Attached[];
  files?: { path: string; file: StashFile }[];
  /// The session's folder, where its files upload to.
  root?: string;
  /// A steer's progress; null before sending.
  state: SteerState | null;
  /// For a sent steer: the session read after which the node holds it.
  after: number;
}

/// A message waiting on this device for its session's machine, as
/// `outbox_list` gives it, without its image data.
export interface OutboxRow {
  id: number;
  how: "prompt" | "steer";
  text: string;
  /// The names of its images.
  images: string[];
  state: "waiting" | "resuming" | "uploading" | "sending" | "failed";
  /// Why the machine refused a failed one.
  reason: string | null;
  /// For a row the outbox just handed to the machine: the keys it gave the images.
  keys: string[];
}

/// What a waiting row says after its text.
export function outboxNote(row: OutboxRow) {
  switch (row.state) {
    case "waiting": return "waiting for connection";
    case "resuming": return "resuming";
    case "uploading": return "uploading";
    case "sending": return "sending";
    case "failed": return row.reason || "not sent";
  }
}

/// What `agent_steer` takes for the files a message uploads before it goes:
/// the files, which go to the machine behind the row, and the other paths it
/// mentions. Null when it has no files.
export interface Attach { root: string; files: { name: string; mime: string; data: string }[]; mentions: string[] }

/// What an ended session needs before a message goes: it resumes, on `model`
/// for Codex, and then takes `pick`.
export interface Resume { model: string | null; pick: { model: string | null; effort: string | null; fast: boolean | null } | null }

/// What `agent_steer` takes beside a message: its files, and the resume of an ended session.
export interface Extras { attach?: Attach | null; resume?: Resume | null }

export function attachOf({ paths, files = [], root = "" }: { paths: readonly string[]; files?: readonly { path: string; file: StashFile }[]; root?: string }): Attach | null {
  if (!files.length) return null;
  const uploaded = new Set(files.map(({ path }) => path));

  return { root, files: files.map(({ file: { name, mime, data } }) => ({ name, mime, data })), mentions: paths.filter(path => !uploaded.has(path)) };
}

/// The row to show for a message the outbox just handed to the node, or null.
/// A page that lists the node's held messages already draws them (see
/// `syncSteers`), so nothing is added then. An older node reports only a
/// count: add a row unless the queue already shows that many.
export function sentFromOutbox(row: OutboxRow, items: Waiting[], page: { queued: number; queued_messages?: string[] } | null, read: number): Waiting | null {
  if (page?.queued_messages && page.queued_messages.length === page.queued) return null;
  if (page && page.queued <= items.filter(q => q.state === "sent").length) return null;

  return { typed: row.text, text: row.text, paths: [], images: [], state: "sent", after: read };
}

/// Marks the steers the node no longer holds as delivered, oldest first.
export function markDelivered(items: Waiting[], waiting: number, read: number) {
  const held = items.filter(q => q.state === "sent" && q.after < read);
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
      typed: text, text, paths: [], images: [], state: "sent", after: read,
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
