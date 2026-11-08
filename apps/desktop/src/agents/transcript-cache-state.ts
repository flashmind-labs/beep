import { joinRuns, type Run } from "./older-state.ts";

export interface TranscriptCursor {
  start: number;
  next: number;
  fingerprint: string;
}

export interface TranscriptUpdate<T extends Run> {
  kind: "snapshot" | "delta";
  page: T;
  cursor: TranscriptCursor;
  resumed: boolean;
}

interface Cached<T extends Run> {
  page: T;
  cursor: TranscriptCursor;
}

// Retain current state across navigation and reconnects. No event history is kept.
export class TranscriptCache<T extends Run> {
  private readonly sessions = new Map<string, Cached<T>>();
  private readonly limit: number;
  private readonly characters: number;

  constructor(limit = 8, characters = 8 * 1024 * 1024) {
    this.limit = limit;
    this.characters = characters;
  }

  get(key: string): Cached<T> | undefined {
    const held = this.sessions.get(key);
    if (held) {
      this.sessions.delete(key);
      this.sessions.set(key, held);
    }
    return held;
  }

  delete(key: string) { this.sessions.delete(key); }

  receive(key: string, update: TranscriptUpdate<T>): T | null {
    const held = this.sessions.get(key);
    const { cursor } = update;
    const append = update.kind === "delta" || update.resumed;
    if (!Number.isSafeInteger(cursor.start) || !Number.isSafeInteger(cursor.next)
      || !Number.isSafeInteger(update.page.earlier) || update.page.earlier < 0
      || cursor.start < 0 || cursor.next < cursor.start
      || update.page.earlier + update.page.entries.length !== cursor.next
      || (append && (!held || update.page.earlier < held.page.earlier
        || update.page.earlier > held.cursor.next
        || (update.resumed && update.page.earlier !== held.cursor.next)))) {
      this.sessions.delete(key);
      return null;
    }
    let page = append ? joinRuns(held!.page, update.page) : update.page;
    if (cursor.start < page.earlier || cursor.start > cursor.next) {
      this.sessions.delete(key);
      return null;
    }
    page = { ...page, earlier: cursor.start, entries: page.entries.slice(cursor.start - page.earlier) };
    this.sessions.delete(key);
    this.sessions.set(key, { page, cursor });
    while (this.sessions.size > this.limit || this.size() > this.characters) {
      const oldest = this.sessions.keys().next().value;
      // One retained window is bounded by the node even when it exceeds the shared budget.
      if (oldest === undefined || this.sessions.size === 1) break;
      this.sessions.delete(oldest);
    }
    return page;
  }

  private size() {
    let characters = 0;
    for (const held of this.sessions.values()) {
      for (const entry of held.page.entries) characters += entry.text.length;
    }
    return characters;
  }
}

export const transcriptKey = (session: { machine_id: string; agent: string; session: string }) =>
  JSON.stringify([session.machine_id, session.agent, session.session]);
