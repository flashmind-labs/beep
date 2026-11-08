// Owns one session subscription and ignores replies from readers already detached.

interface Reader<K> {
  key: K;
  id: string;
  live: boolean;
  version: number;
  retryAt: number;
  ended: boolean;
  startedAt: number;
}

interface Transport<K, T> {
  id(): string;
  now(): number;
  open(key: K, id: string, receive: (update: T) => void): Promise<void>;
  close(id: string): void;
  receive(key: K, update: T): void;
  disconnected(key: K, error: unknown): void;
}

export class AgentFeed<K, T> {
  private reader: Reader<K> | null = null;

  private readonly transport: Transport<K, T>;
  private serial = 0;

  constructor(transport: Transport<K, T>) { this.transport = transport; }

  live(key: K) { return this.reader?.key === key && this.reader.live; }
  version(key: K) { return this.reader?.key === key ? this.reader.version : 0; }
  waiting(key: K) {
    const reader = this.reader;
    return reader?.key === key && !reader.live && !reader.ended
      && this.transport.now() - reader.startedAt < 3000;
  }

  follow(key: K) {
    const current = this.reader;
    if (current?.key === key && (!current.ended || this.transport.now() < current.retryAt)) return;
    this.stop();
    const reader: Reader<K> = { key, id: this.transport.id(), live: false, version: ++this.serial, retryAt: 0, ended: false, startedAt: this.transport.now() };
    this.reader = reader;
    const receive = (update: T) => {
      if (this.reader !== reader || reader.ended) return;
      reader.live = true;
      reader.version = ++this.serial;
      this.transport.receive(key, update);
    };
    void this.transport.open(key, reader.id, receive).then(
      () => this.disconnected(reader, "Session stream disconnected."),
      error => this.disconnected(reader, error),
    );
  }

  private disconnected(reader: Reader<K>, error: unknown) {
    if (this.reader !== reader) return;
    const wasLive = reader.live;
    reader.live = false;
    reader.ended = true;
    reader.retryAt = this.transport.now() + (wasLive ? 5000 : 60_000);
    if (wasLive) this.transport.disconnected(reader.key, error);
  }

  stop() {
    const reader = this.reader;
    this.reader = null;
    if (reader) this.transport.close(reader.id);
  }
}
