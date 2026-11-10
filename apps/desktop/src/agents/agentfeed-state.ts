// Owns a window's session subscriptions: the one on screen, and others kept
// while they run or ask. It ignores replies from readers already detached.

/// What a followed session is doing, which decides how long it stays followed off screen.
export type Activity = "running" | "asking" | "idle";

/// How long a session off screen stays followed once it is idle.
export const IDLE_FOLLOW_MS = 5 * 60_000;
/// How many sessions off screen stay followed at once.
export const BACKGROUND_FOLLOWS = 8;

interface Reader<K> {
  key: K;
  name: string;
  id: string;
  live: boolean;
  version: number;
  retryAt: number;
  ended: boolean;
  startedAt: number;
  /// When it was last on screen, as a serial: the lowest is dropped first.
  viewed: number;
  activity: Activity;
  idleSince: number;
  /// Cancels the timer that unfollows it once idle off screen.
  idle: (() => void) | null;
}

interface Transport<K, T> {
  id(): string;
  now(): number;
  name(key: K): string;
  later(ms: number, run: () => void): () => void;
  open(key: K, id: string, receive: (update: T) => void): Promise<void>;
  close(id: string): void;
  /// `key` is no longer followed, for whatever reason it stopped.
  unfollowed(key: K): void;
  /// The reader whose session is on screen, or none.
  show(id: string | null): void;
  receive(key: K, update: T): void;
  disconnected(key: K, error: unknown): void;
}

export class AgentFeed<K, T> {
  private readonly readers = new Map<string, Reader<K>>();
  private shown: string | null = null;
  private shownId: string | null = null;

  private readonly transport: Transport<K, T>;
  private serial = 0;
  private views = 0;

  constructor(transport: Transport<K, T>) { this.transport = transport; }

  private reader(key: K) { return this.readers.get(this.transport.name(key)); }

  live(key: K) { return !!this.reader(key)?.live; }
  version(key: K) { return this.reader(key)?.version ?? 0; }
  waiting(key: K) {
    const reader = this.reader(key);
    return !!reader && !reader.live && !reader.ended
      && this.transport.now() - reader.startedAt < 3000;
  }

  /// The sessions followed, the one on screen included.
  followed() { return [...this.readers.keys()]; }

  /// Follows `key` as the session on screen. The one shown before stays
  /// followed off screen while it runs or asks.
  follow(key: K) {
    const name = this.transport.name(key);
    if (this.shown !== name) this.release();
    this.shown = name;
    let reader = this.readers.get(name);

    if (reader?.ended && this.transport.now() >= reader.retryAt) {
      this.remove(reader);
      reader = undefined;
    }

    reader ??= this.open(key, name);
    reader.key = key;
    reader.viewed = ++this.views;
    reader.idle?.();
    reader.idle = null;
    this.showReader(reader.id);
    this.limit();
  }

  /// The session on screen leaves it: the window hid or another view opened.
  leave() {
    this.release();
    this.showReader(null);
    this.limit();
  }

  private release() {
    const reader = this.shown === null ? undefined : this.readers.get(this.shown);
    this.shown = null;
    if (reader) this.settle(reader);
  }

  /// What `key` is doing now, from its newest update.
  activity(key: K, activity: Activity) {
    const reader = this.reader(key);
    if (!reader || reader.activity === activity) return;
    if (activity === "idle") reader.idleSince = this.transport.now();
    reader.activity = activity;
    if (reader.name !== this.shown) this.settle(reader);
  }

  /// Stops following `key`, as after a gap in its updates.
  drop(key: K) {
    const reader = this.reader(key);
    if (reader) this.remove(reader);
  }

  private remove(reader: Reader<K>) {
    if (this.readers.get(reader.name) !== reader) return;
    this.readers.delete(reader.name);
    reader.idle?.();
    reader.idle = null;
    if (reader.id === this.shownId) this.showReader(null);
    this.transport.close(reader.id);
    this.transport.unfollowed(reader.key);
  }

  /// Stops following every session.
  clear() {
    for (const reader of [...this.readers.values()]) this.remove(reader);
    this.shown = null;
  }

  /// Opens every subscription again from where it was, after the OS
  /// suspended the app and its links may have dropped.
  resync() {
    for (const old of [...this.readers.values()]) {
      this.transport.close(old.id);
      old.idle?.();
      const reader = this.open(old.key, old.name);
      Object.assign(reader, { viewed: old.viewed, activity: old.activity, idleSince: old.idleSince });
      if (old.name === this.shown) this.showReader(reader.id);
      else this.settle(reader);
    }
  }

  private open(key: K, name: string) {
    const now = this.transport.now();
    const reader: Reader<K> = {
      key, name, id: this.transport.id(), live: false, version: ++this.serial, retryAt: 0, ended: false, startedAt: now,
      viewed: 0, activity: "idle", idleSince: now, idle: null,
    };
    this.readers.set(name, reader);
    const receive = (update: T) => {
      if (this.readers.get(name) !== reader || reader.ended) return;
      reader.live = true;
      reader.version = ++this.serial;
      this.transport.receive(reader.key, update);
    };
    void this.transport.open(key, reader.id, receive).then(
      () => this.disconnected(reader, "Session stream disconnected."),
      error => this.disconnected(reader, error),
    );
    this.limit();
    return reader;
  }

  private showReader(id: string | null) {
    if (this.shownId === id) return;
    this.shownId = id;
    this.transport.show(id);
  }

  /// A session off screen: an idle one is unfollowed `IDLE_FOLLOW_MS` after
  /// it went idle, one that runs or asks stays.
  private settle(reader: Reader<K>) {
    if (reader.ended) { this.remove(reader); return; }

    if (reader.activity !== "idle") {
      reader.idle?.();
      reader.idle = null;
      return;
    }

    const left = reader.idleSince + IDLE_FOLLOW_MS - this.transport.now();
    if (left <= 0) { this.remove(reader); return; }
    reader.idle ??= this.transport.later(left, () => { reader.idle = null; this.remove(reader); });
  }

  /// Past `BACKGROUND_FOLLOWS` off screen, the least recently viewed goes,
  /// one that asks only when every other one asks too.
  private limit() {
    const hidden = [...this.readers.values()].filter(reader => reader.name !== this.shown);

    while (hidden.length > BACKGROUND_FOLLOWS) {
      const order = (reader: Reader<K>) => [reader.activity === "asking" ? 1 : 0, reader.viewed];
      hidden.sort((a, b) => { const [x, y] = [order(a), order(b)]; return x[0] - y[0] || x[1] - y[1]; });
      const [oldest] = hidden.splice(0, 1);
      this.remove(oldest);
    }
  }

  private disconnected(reader: Reader<K>, error: unknown) {
    if (this.readers.get(reader.name) !== reader) return;
    const wasLive = reader.live;
    reader.live = false;
    reader.ended = true;
    reader.retryAt = this.transport.now() + (wasLive ? 5000 : 60_000);

    // Off screen it reads again from where it was once opened.
    if (reader.name !== this.shown) { this.remove(reader); return; }

    if (wasLive) this.transport.disconnected(reader.key, error);
  }
}
