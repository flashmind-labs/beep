import { AG } from "./agents-state.ts";
import type { Attached } from "./agentqueue.ts";
import { conversationKey } from "./conversation-state.ts";
import { agentMessage } from "./session-frame-state.ts";
import type { Transfer } from "../agentpane/files-state.ts";
import type { StashFile } from "../chat/stash-state.ts";

// The paths and files a session's next message carries: the state of its mentions and uploads.

/// A file on its way to the session's machine.
export interface Upload extends Transfer {
  name: string;
}

/// One attachment batch: files being read, or uploaded to a folder; `size`
/// and `done` count the bytes of all its files.
export interface UploadBatch {
  /// Files only read from the disk, which go with the message: its send waits for them.
  local: boolean;
  /// What a local batch attached: the keys of its files, and its pictures.
  paths: string[];
  images: Attached[];
  ready: Promise<void>;
  size: number;
  done: number;
}

/// Files dropped on a folder, waiting for the user to say where they go.
interface UploadAsk {
  files: File[];
  folder: string;
  root: string;
}

export interface MentionState {
  paths: string[];
  uploads: Upload[];
  pending: Set<UploadBatch>;
  ask: UploadAsk | null;
  files: Map<string, StashFile>;
  reading: number;
  images: Attached[];
  error: string;
}

/// How long a failed upload stays said, as the field shows it (agentAttachError).
export const ERROR_SHOWN_MS = 5000;
const freshMentions = (): MentionState => ({ paths: [], uploads: [], pending: new Set(), ask: null, files: new Map(), reading: 0, images: [], error: "" });
const conversations = new Map<string, MentionState>();
let current = freshMentions();
export const currentMentions = () => current;
export const mentionImages = (state: MentionState) => state === current ? AG.att : state.images;
export function setMentionImages(state: MentionState, images: Attached[]) {
  state.images = images;
  if (state === current) setAttached(images);
}

/// Replaces the images attached to the open conversation's message: the one place `AG.att` is set.
export function setAttached(images: Attached[]) { AG.att = images; }

/// Takes image `i` off the open message, in place.
export function removeAttached(i: number) { return AG.att.splice(i, 1)[0]; }

/// What went wrong attaching, under the field: the one place `AG.attError` is set.
export function setAttachError(text: string) { AG.attError = text; }
export function rememberMentionImages() { current.images = AG.att; }

export function mentionsOf(session: { machine_id: string; agent: string; id: string }) {
  const key = conversationKey(session);
  let state = conversations.get(key);
  if (!state) { state = freshMentions(); conversations.set(key, state); }
  return state;
}

export function activateMentions(session: { machine_id: string; agent: string; id: string }) {
  const state = mentionsOf(session);
  if (state === current) { setAttachError(state.error); return; }
  current = state;
  setAttached(state.images);
  setAttachError(state.error);
}

export const MN = {
  get paths() { return current.paths; },
  set paths(value: MentionState["paths"]) { current.paths = value; },
  get uploads() { return current.uploads; },
  set uploads(value: MentionState["uploads"]) { current.uploads = value; },
  get pending() { return current.pending; },
  set pending(value: MentionState["pending"]) { current.pending = value; },
  get ask() { return current.ask; },
  set ask(value: MentionState["ask"]) { current.ask = value; },
  get files() { return current.files; },
  set files(value: MentionState["files"]) { current.files = value; },
  get reading() { return current.reading; },
  set reading(value: MentionState["reading"]) { current.reading = value; },
  redraw: () => {},
  error: (_text: string) => {},
};

export function resetMentions() { current = freshMentions(); }

export function redrawUploads(state: MentionState) {
  if (state === current) MN.redraw();
}

/// Mentions `path` (relative to the session's folder) in the next message.
export function addMention(path: string) {
  const p = path.replace(/^\.\/+/, "").replace(/\/+$/, "");
  if (p && !MN.paths.includes(p)) MN.paths.push(p);
  MN.redraw();
  agentMessage.current?.focus();
}

export function removeMention(index: number) {
  if (index < 0) return;
  MN.paths.splice(index, 1);
  MN.redraw();
  agentMessage.current?.focus();
}

/// Files dropped on the folder `folder` of the Files tab: asked first.
export function askUpload(files: File[], folder: string, root: string) {
  if (!files.length) return;
  MN.ask = { files, folder, root };
  MN.redraw();
}

/// The key of a file attached to the next message and not uploaded yet: its
/// path in `MN.paths`, and its entry in `MN.files`.
export const attachmentKey = (name: string) => `attachment:${++attachments}:${name}`;
let attachments = 0;

/// The largest file a message uploads; the app refuses larger ones too.
export const MAX_ATTACH_BYTES = 512 * 1024 * 1024;

/// The batches of files being read, which a message sent now takes once they are.
export const attachReads = () => [...current.pending].filter(batch => batch.local);
