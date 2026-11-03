// What a project's page last read, section by section, so an opened
// project draws at once from it and then reads again quietly. The window
// keeps it in memory while it lives and hands it to the app to save, so a
// fresh start shows the last known data too. It belongs to one account:
// a file saved for another is dropped, and so is everything on sign-out.
// Pure state, no DOM, so the tests check it.

/// A project page section the cache keeps: the project chat's view, where
/// the project lives, its agent sessions, its pull requests and issues, its
/// branches, its workflow runs, its terminals and its file sync.
export type PageSection = "view" | "where" | "sessions" | "threads" | "branches" | "runs" | "terminals" | "filesync";

const SECTIONS: readonly PageSection[] = ["view", "where", "sessions", "threads", "branches", "runs", "terminals", "filesync"];

/// One kept answer: the data, when it was read, and when it was last read
/// or used, which decides what goes first once the cache is too big.
interface Slot {
  data: unknown;
  at: number;
  used: number;
}

/// The cache as the app saves it.
export interface PageCacheFile {
  format: number;
  account: string;
  slots: Record<string, Slot>;
}

/// Raised whenever the saved layout changes.
export const PAGE_CACHE_FORMAT = 1;
/// How often a section reads again from the client, at most, unless the
/// user asks with Refresh. Pull requests, issues and branches come from the
/// code host through the server, so this stays at a few minutes.
export const REFRESH_MS = 3 * 60_000;
/// Where the project lives and its sessions come from the machines, not
/// the code host, and change as agents start, so they read sooner.
export const MACHINE_REFRESH_MS = 60_000;

/// Opening a project reads what the server keeps of its code host (pull
/// requests, issues, runs) again once it is this old, so an issue closed
/// or a run started since shows. It is the server's copy: the host itself
/// is read at most every few minutes whatever the window asks.
export const SWITCH_MS = 20_000;

/// Each section's `REFRESH_MS` or `MACHINE_REFRESH_MS`. The project
/// chat's view follows the window's own refresh, so it is never due here.
export const REFRESH: Record<PageSection, number> = {
  view: REFRESH_MS,
  where: MACHINE_REFRESH_MS,
  sessions: MACHINE_REFRESH_MS,
  threads: REFRESH_MS,
  branches: REFRESH_MS,
  runs: MACHINE_REFRESH_MS,
  terminals: MACHINE_REFRESH_MS,
  filesync: MACHINE_REFRESH_MS,
};
/// Answers kept at most, across every project and section.
export const MAX_SLOTS = 150;
/// The saved file's size at most, in characters of JSON.
export const MAX_CHARS = 1_000_000;
/// An answer older than this is not shown at all.
export const MAX_AGE_MS = 14 * 86_400_000;
/// Data younger than this needs no note on its section.
export const QUIET_MS = 60_000;

function slotKey(section: PageSection, id: string) { return `${section}\n${id}`; }

function isSlot(v: unknown): v is Slot {
  if (!v || typeof v !== "object") return false;
  const s = v as Record<string, unknown>;
  return "data" in s && typeof s.at === "number" && typeof s.used === "number";
}

function knownKey(key: string) {
  const at = key.indexOf("\n");
  return at > 0 && (SECTIONS as readonly string[]).includes(key.slice(0, at));
}

/// The slots to keep, most recently used first: at most `maxSlots`, whose
/// JSON together stays within `maxChars`. One alone too big is dropped.
export function evict(slots: readonly [string, Slot][], maxSlots = MAX_SLOTS, maxChars = MAX_CHARS): [string, Slot][] {
  const out: [string, Slot][] = [];
  let chars = 0;
  for (const [key, slot] of [...slots].sort((a, b) => b[1].used - a[1].used)) {
    if (out.length >= maxSlots) break;
    const size = key.length + JSON.stringify(slot).length + 4;
    if (chars + size > maxChars) continue;
    chars += size;
    out.push([key, slot]);
  }
  return out;
}

/// What a section's header says about its data: a spinner while it reads
/// data that is not new, else how old the data is once that matters.
export interface Freshness {
  spin: boolean;
  text: string;
}

/// The header note for data read at `at` (undefined: none yet), `loading`
/// while a read is on its way. Data under `QUIET_MS` old says nothing, so
/// the quick polls of an open page never flash a spinner.
export function freshness(at: number | undefined, loading: boolean, now: number): Freshness {
  const old = at === undefined ? Infinity : now - at;
  if (loading && old >= QUIET_MS) return { spin: true, text: "" };
  if (at === undefined || old < QUIET_MS) return { spin: false, text: "" };
  const min = Math.round(old / 60_000);
  if (min < 60) return { spin: false, text: `updated ${min} min ago` };
  const hours = Math.round(old / 3_600_000);
  if (hours < 24) return { spin: false, text: `updated ${hours} h ago` };
  return { spin: false, text: `updated ${Math.round(old / 86_400_000)} d ago` };
}

/// One section's slice of the cache, as threads.ts and review.ts take it.
export interface SectionStore<T> {
  get(id: string): T | undefined;
  put(id: string, data: T): void;
  /// When it was read, undefined for never.
  at(id: string): number | undefined;
  /// Whether it is time to read it again, `every` after the last read
  /// (the section's own time by default).
  due(id: string, every?: number): boolean;
}

export class PageCache {
  private slots = new Map<string, Slot>();
  private changed = false;
  account: string | null;
  private clock: () => number;

  /// `account` is the account the cache belongs to; null keeps it in
  /// memory only. `clock` gives the time, for the tests.
  constructor(account: string | null, clock: () => number = Date.now) {
    this.account = account;
    this.clock = clock;
  }

  /// The cache from a saved `file`, or an empty one when the file is for
  /// another account or format, or does not parse. Answers past
  /// `MAX_AGE_MS` stay behind.
  static from(file: unknown, account: string | null, clock: () => number = Date.now): PageCache {
    const cache = new PageCache(account, clock);
    cache.restore(file, account);
    return cache;
  }

  /// Takes `account` and adds what `file` kept for it, keeping anything
  /// read since that is newer. A file for another account adds nothing.
  restore(file: unknown, account: string | null) {
    this.setAccount(account);
    if (!account || !file || typeof file !== "object") return;
    const f = file as Partial<PageCacheFile>;
    if (f.format !== PAGE_CACHE_FORMAT || f.account !== account || !f.slots || typeof f.slots !== "object") return;
    const now = this.clock();
    for (const [key, slot] of Object.entries(f.slots)) {
      if (!knownKey(key) || !isSlot(slot) || now - slot.at >= MAX_AGE_MS) continue;
      const had = this.slots.get(key);
      if (!had || had.at < slot.at) this.slots.set(key, slot);
    }
  }

  /// The data kept for `section` of `id`.
  get<T>(section: PageSection, id: string): T | undefined {
    const slot = this.slots.get(slotKey(section, id));
    if (!slot) return undefined;
    slot.used = this.clock();
    return slot.data as T;
  }

  /// When `section` of `id` was read, undefined for never.
  at(section: PageSection, id: string): number | undefined {
    return this.slots.get(slotKey(section, id))?.at;
  }

  /// Keeps `data` as what `section` of `id` read now.
  put(section: PageSection, id: string, data: unknown) {
    const now = this.clock();
    this.slots.set(slotKey(section, id), { data, at: now, used: now });
    this.changed = true;
  }

  /// Whether `section` of `id` is due another read: never read, or read
  /// `every` ago or longer.
  due(section: PageSection, id: string, every = REFRESH[section]): boolean {
    const at = this.at(section, id);
    return at === undefined || this.clock() - at >= every;
  }

  /// Forgets everything, on sign-out or when the account changes.
  clear() {
    this.changed = this.slots.size > 0 || this.changed;
    this.slots.clear();
  }

  /// Moves to `account`. Another account, or none (signed out), starts
  /// empty; learning the account of what was read without one keeps it.
  setAccount(account: string | null) {
    if (account === this.account) return;
    if (this.account !== null || account === null) this.clear();
    this.account = account;
  }

  get size() { return this.slots.size; }

  /// True once since the last call when something changed, so the window
  /// saves only then.
  takeChanged(): boolean {
    const was = this.changed;
    this.changed = false;
    return was;
  }

  /// The file to save, within the caps; null without an account. What did
  /// not fit is dropped from memory too.
  toFile(maxSlots = MAX_SLOTS, maxChars = MAX_CHARS): PageCacheFile | null {
    if (!this.account) return null;
    const kept = evict([...this.slots], maxSlots, maxChars);
    if (kept.length < this.slots.size) this.slots = new Map(kept);
    return { format: PAGE_CACHE_FORMAT, account: this.account, slots: Object.fromEntries(kept) };
  }

  /// `section` as a store of its own.
  section<T>(section: PageSection): SectionStore<T> {
    return {
      get: id => this.get<T>(section, id),
      put: (id, data) => this.put(section, id, data),
      at: id => this.at(section, id),
      due: (id, every) => this.due(section, id, every),
    };
  }
}

/// What a background tick of the hidden window does: `run` reads, `woke`
/// skips one that came far too late because the Mac slept (the reconnect
/// that follows reads anyway), `offline` skips while the connection is down.
export type Tick = "run" | "woke" | "offline";

export function backgroundTick(last: number, now: number, every: number, online: boolean): Tick {
  if (now - last > every * 2) return "woke";
  return online ? "run" : "offline";
}
