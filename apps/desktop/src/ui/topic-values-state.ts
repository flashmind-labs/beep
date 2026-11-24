// The last value a topic pushed, per machine, so a view shows it at once and then follows updates.
import { type MeshEvent, subscribeEach } from "./mesh-events.ts";
import { normalizeTopic, type Topic } from "./topic-state.ts";

/// Keeps what `parse` accepts from each machine's updates for exact topics, the least recently
/// used dropped past `limit`. A value is kept after its last view leaves, so the view that comes
/// back shows it while the node sends the current one.
export class TopicValues<T> {
  private readonly kept = new Map<string, T>();
  private readonly parse: (data: unknown) => T | null;
  private readonly limit: number;

  /// `parse` returns null for data that is not this topic's value; it is dropped and the last value stays.
  constructor(parse: (data: unknown) => T | null, limit: number) {
    this.parse = parse;
    this.limit = limit;
  }

  /// The kept value, which counts as used.
  get(machine: string, topic: Topic): T | null {
    const key = keyOf(machine, normalizeTopic(topic));
    const value = this.kept.get(key);
    if (value === undefined) return null;

    this.kept.delete(key);
    this.kept.set(key, value);

    return value;
  }

  /// Calls `receive` with the kept value now, if any, then with each update as it arrives.
  /// Native code retains and resends only what a window did not already have, so a second view of
  /// the same topic gets its first value from here.
  watch(machine: string, topic: Topic, receive: (value: T) => void, when?: () => boolean): () => void {
    const name = normalizeTopic(topic);
    const stop = subscribeEach(name, event => {
      const value = this.keep(machine, name, event);
      if (value !== null) receive(value);
    }, { machine, when });
    const kept = this.get(machine, name);
    if (kept !== null) receive(kept);

    return stop;
  }

  /// Replaces the kept value, for a change the owner made that the node has not pushed yet.
  put(machine: string, topic: Topic, value: T) {
    this.store(keyOf(machine, normalizeTopic(topic)), value);
  }

  private store(key: string, value: T) {
    this.kept.delete(key);
    this.kept.set(key, value);
    const oldest = this.kept.keys().next();
    if (this.kept.size > this.limit && !oldest.done) this.kept.delete(oldest.value);
  }

  private keep(machine: string, topic: Topic, event: MeshEvent): T | null {
    // An answer to a query, not the topic's own update, and updates of other resources under it.
    if (event.query != null || event.machine_id !== machine || event.topic !== topic) return null;
    const value = this.parse(event.data);

    if (value === null) {
      console.warn("topic value not understood:", topic);
      return null;
    }

    this.store(keyOf(machine, topic), value);

    return value;
  }
}

const keyOf = (machine: string, topic: Topic) => `${machine}\0${topic}`;
