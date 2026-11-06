// Sessions the user closed, by machine and id: "closing" from the click until
// the machine says the session ended, then "ended" until the lists read it so.

export type CloseState = "closing" | "ended";
type Mark = { state: CloseState; at: number };
type Ref = { machine_id: string; id: string };

/// How long a mark stands in for the lists: a close ends within seconds and
/// the next read shows it.
export const MARK_MS = 60_000;

export class Closings {
  private marks = new Map<string, Mark>();

  private static key(s: Ref) { return `${s.machine_id}:${s.id}`; }

  /// The close of `s` was sent.
  start(s: Ref, now: number) { this.marks.set(Closings.key(s), { state: "closing", at: now }); }

  /// The machine refused the close: the row is as it was.
  refused(s: Ref) { this.marks.delete(Closings.key(s)); }

  /// The machine said session `id` ended; true when one was closing.
  ended(id: string, now: number) {
    let any = false;
    for (const [key, mark] of this.marks) {
      if (mark.state === "closing" && key.endsWith(`:${id}`)) {
        this.marks.set(key, { state: "ended", at: now });
        any = true;
      }
    }
    return any;
  }

  stateOf(s: Ref, now: number): CloseState | null {
    const key = Closings.key(s), mark = this.marks.get(key);
    if (!mark) return null;
    if (now - mark.at > MARK_MS) { this.marks.delete(key); return null; }
    return mark.state;
  }
}
