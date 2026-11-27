// Messages typed for a coding agent, waiting on this device or held by its node.
import type { StashFile } from "../chat/stash-state.ts";

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

/// A message taken from the field.
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
  /// Set while its files are still being read: the outbox keeps the words under this token,
  /// and `outbox_attach` completes the message.
  reading?: string;
}

/// A message waiting on this device for its session's machine, as
/// `outbox_list` gives it, without its image data.
export interface OutboxRow {
  id: number;
  how: "prompt" | "steer";
  text: string;
  /// The names of its images.
  images: string[];
  state: "waiting" | "reading" | "resuming" | "uploading" | "sending" | "sent" | "failed";
  /// Why the machine refused a failed one.
  reason: string | null;
  /// For a row the outbox just handed to the machine: the keys it gave the images.
  keys: string[];
}

/// What a waiting row says after its text.
export function outboxNote(row: OutboxRow) {
  switch (row.state) {
    case "waiting": return "waiting for connection";
    case "reading": return "reading files";
    case "resuming": return "resuming";
    case "uploading": return "uploading";
    case "sending": return "sending";
    // The machine has it; its own list shows it from the next update.
    case "sent": return row.how === "steer" ? "after the current step" : "sent";
    case "failed": return row.reason || "not sent";
  }
}

/// What `agent_steer` takes for the files a message uploads before it goes:
/// the files, which go to the machine behind the row, and the other paths it
/// mentions. Null when it has no files.
export interface Attach { root: string; files: { name: string; mime: string; data: string }[]; mentions: string[]; reading?: string }

/// What an ended session needs before a message goes: it resumes, on `model`
/// for Codex, and then takes `pick`.
export interface Resume { model: string | null; pick: { model: string | null; effort: string | null; fast: boolean | null } | null }

/// What `agent_steer` takes beside a message: its files, and the resume of an ended session.
export interface Extras { attach?: Attach | null; resume?: Resume | null }

export function attachOf({ paths, files = [], root = "", reading }: { paths: readonly string[]; files?: readonly { path: string; file: StashFile }[]; root?: string; reading?: string }): Attach | null {
  if (reading) return { root, files: [], mentions: [], reading };
  if (!files.length) return null;
  const uploaded = new Set(files.map(({ path }) => path));

  return { root, files: files.map(({ file: { name, mime, data } }) => ({ name, mime, data })), mentions: paths.filter(path => !uploaded.has(path)) };
}

/// A message the node holds for the turn, as its list gives it: the words, and the names of
/// the pictures that go with it.
export function heldMessage(text: string) {
  const lines = text.split("\n"), names: string[] = [];
  const words = lines.filter(line => {
    const image = /^\[image: (.+)\]$/i.exec(line.trim());
    if (image) names.push(image[1]);
    return !image;
  });

  return { words: words.join("\n").trim(), names };
}

/// A stop ends when its turn finishes, even if the next turn starts before a read.
export function stopPending(stop: { id: string; turns: number } | null, id: string, active: boolean, turns: number) {
  return stop?.id === id && active && turns <= stop.turns ? stop : null;
}
