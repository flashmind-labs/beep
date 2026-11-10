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
  private readonly entrySizes = new WeakMap<object, number>();
  /// Each held page's size in characters, by key.
  private readonly sizes = new Map<string, number>();

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

  /// What is held for `key`, without making it the most recent.
  peek(key: string): Cached<T> | undefined { return this.sessions.get(key); }

  /// Holds a page kept from an earlier run, unless a newer one is held already.
  seed(key: string, kept: Cached<T>) {
    if (!this.sessions.has(key)) this.sessions.set(key, kept);
  }

  delete(key: string) {
    this.sessions.delete(key);
    this.sizes.delete(key);
  }

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
      this.delete(key);
      return null;
    }

    let page = append ? joinRuns(held!.page, update.page) : update.page;

    if (cursor.start < page.earlier || cursor.start > cursor.next) {
      this.delete(key);
      return null;
    }

    page = { ...page, earlier: cursor.start, entries: page.entries.slice(cursor.start - page.earlier) };
    const size = this.pageSize(key, append ? held ?? null : null, update.page.earlier, page);
    this.sessions.delete(key);
    this.sessions.set(key, { page, cursor });
    this.sizes.set(key, size);

    while (this.sessions.size > this.limit || this.size() > this.characters) {
      const oldest = this.sessions.keys().next().value;
      // One retained window is bounded by the node even when it exceeds the shared budget.
      if (oldest === undefined || this.sessions.size === 1) break;
      this.delete(oldest);
    }

    return page;
  }

  private size() {
    let characters = 0;

    for (const [key, held] of this.sessions) {
      let size = this.sizes.get(key);
      if (size === undefined) this.sizes.set(key, size = this.entriesSize(held.page.entries));
      characters += size;
    }

    return characters;
  }

  /// The size of `page`, which took the entries of `held` before `from` and
  /// the update's from there: only the entries it took or dropped are counted.
  private pageSize(key: string, held: Cached<T> | null, from: number, page: T) {
    const before = this.sizes.get(key), h = held?.page;
    if (!h || before === undefined || page.earlier < h.earlier || page.earlier > from) return this.entriesSize(page.entries);
    return before - this.entriesSize(h.entries.slice(0, page.earlier - h.earlier)) - this.entriesSize(h.entries.slice(from - h.earlier))
      + this.entriesSize(page.entries.slice(from - page.earlier));
  }

  private entriesSize(entries: readonly T["entries"][number][]) {
    let characters = 0;

    for (const entry of entries) {
      let size = this.entrySizes.get(entry);

      if (size === undefined) {
        // Include tool payloads while reusing measured immutable prefix entries.
        const plain = JSON.stringify({ kind: entry.kind, text: entry.text }).length;
        size = entry.text.length + Math.max(0, JSON.stringify(entry).length - plain);
        this.entrySizes.set(entry, size);
      }

      characters += size;
    }

    return characters;
  }
}

export const transcriptKey = (session: { machine_id: string; agent: string; session: string }) =>
  JSON.stringify([session.machine_id, session.agent, session.session]);
