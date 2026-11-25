import { conversationKey } from "./conversation-state.ts";
import type { AgentModes, AgentPage, AgentTask, OpenAgent } from "./agents-state.ts";
import { BACKGROUND_FOLLOWS } from "./agentfeed-state.ts";
import { joinRuns, type Run } from "./older-state.ts";

/// A task that has not ended.
export const runs = (t: AgentTask) => t.state === "running" || t.state === "paused";

const NO_TASKS: AgentTask[] = [];

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
  private readonly listeners = new Set<() => void>();
  /// The newest modes and tasks of each session, by key: kept beside its page, and like it
  /// dropped when the session is evicted or forgotten, not when a gap drops the page.
  private readonly extras = new Map<string, { modes: AgentModes | null; tasks: AgentTask[] }>();

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

  /// `listener` runs after any held page changes; for `useSyncExternalStore`.
  subscribe = (listener: () => void) => {
    this.listeners.add(listener);
    return () => { this.listeners.delete(listener); };
  };

  private changed() { for (const listener of [...this.listeners]) listener(); }

  /// What is held for `key`, without making it the most recent.
  peek(key: string): Cached<T> | undefined { return this.sessions.get(key); }

  /// Holds a page kept from an earlier run, unless a newer one is held already.
  seed(key: string, kept: Cached<T>) {
    if (this.sessions.has(key)) return;
    this.sessions.set(key, kept);
    this.changed();
  }

  /// Replaces the page held for `key` with what `change` makes of it, in the same place in the order,
  /// and tells the listeners; nothing when none is held.
  amend(key: string, change: (page: T) => T) {
    const held = this.sessions.get(key);
    if (!held) return;
    this.sessions.set(key, { ...held, page: change(held.page) });
    this.changed();
  }

  /// Drops the page held for `key`, as after a gap; its modes and tasks stay for the next update.
  private drop(key: string) {
    this.sessions.delete(key);
    this.sizes.delete(key);
    this.changed();
  }

  delete(key: string) {
    this.extras.delete(key);
    this.drop(key);
  }

  modes(key: string) { return this.extras.get(key)?.modes ?? null; }
  tasks(key: string) { return this.extras.get(key)?.tasks ?? NO_TASKS; }

  setModes(key: string, modes: AgentModes | null) {
    this.extras.set(key, { modes, tasks: this.tasks(key) });
    this.changed();
  }

  setTasks(key: string, tasks: AgentTask[]) {
    this.extras.set(key, { modes: this.modes(key), tasks });
    this.changed();
  }

  /// Forgets the modes and tasks of `key`, which is no longer followed and would go stale.
  clearExtras(key: string) {
    if (this.extras.delete(key)) this.changed();
  }

  receive(key: string, update: TranscriptUpdate<T> & { modes?: AgentModes | null; tasks?: AgentTask[] }): T | null {
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
      this.drop(key);
      return null;
    }

    let page = append ? joinRuns(held!.page, update.page) : update.page;

    if (cursor.start < page.earlier || cursor.start > cursor.next) {
      this.drop(key);
      return null;
    }

    page = { ...page, earlier: cursor.start, entries: page.entries.slice(cursor.start - page.earlier) };
    const size = this.pageSize(key, append ? held ?? null : null, update.page.earlier, page);
    this.sessions.delete(key);
    this.sessions.set(key, { page, cursor });
    this.sizes.set(key, size);
    if (update.modes !== undefined && update.tasks) this.extras.set(key, { modes: update.modes, tasks: update.tasks });

    while (this.sessions.size > this.limit || this.size() > this.characters) {
      const oldest = this.sessions.keys().next().value;
      // One retained window is bounded by the node even when it exceeds the shared budget.
      if (oldest === undefined || this.sessions.size === 1) break;
      this.delete(oldest);
    }

    this.changed();

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

export const transcriptKey = (session: OpenAgent) =>
  conversationKey({ machine_id: session.machine_id, agent: session.agent, id: session.session });

// Room for the open session, those followed off screen and a few recent.
export const transcripts = new TranscriptCache<AgentPage>(BACKGROUND_FOLLOWS + 4);
