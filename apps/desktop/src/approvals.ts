// What approval cards show, kept apart from the DOM so it can be tested:
// which approvals still wait, and the logo of the app that asks.

/// Approvals answered here that the server may still list, by the chat
/// they are in. A list fetched before the answer landed, or read before the
/// resumed run saved the tool's result, would bring the card back; `keep`
/// leaves those out. An id is forgotten once its chat's list leaves it
/// out, which says the server has the answer.
export class AnsweredIds {
  /// Query id to chat.
  private ids = new Map<string, string>();

  mark(queryId: string, chat: string | null) { this.ids.set(queryId, chat ?? ""); }
  /// The answer did not go through: the card may show again.
  forget(queryId: string) { this.ids.delete(queryId); }
  has(queryId: string) { return this.ids.has(queryId); }

  /// What of `listed`, chat `chat`'s whole list, still waits, by `idOf`.
  keep<T>(chat: string | null, listed: T[], idOf: (item: T) => string): T[] {
    const present = new Set(listed.map(idOf));
    for (const [id, of] of this.ids) if (of === (chat ?? "") && !present.has(id)) this.ids.delete(id);
    return listed.filter(item => !this.ids.has(idOf(item)));
  }
}

/// An approval the orb can show (Rust's `Waiting`).
export interface Asked {
  session: string;
  approval: { query_id: string };
}

/// The approvals the orb shows while live mode is on, oldest first; the
/// first is on the card. One answered here never comes back, however late
/// an event or a list repeats it.
export class OrbAsks<W extends Asked> {
  private list: W[] = [];
  private answered = new AnsweredIds();

  get shown(): W | null { return this.list[0] ?? null; }
  get count() { return this.list.length; }

  /// Adds `w` unless it is there already or was answered. True when it
  /// was added.
  add(w: W): boolean {
    const id = w.approval.query_id;
    if (this.answered.has(id) || this.list.some(x => x.approval.query_id === id)) return false;
    this.list.push(w);
    return true;
  }

  /// Takes `queryId` away, answered or withdrawn elsewhere. True when it
  /// was there.
  remove(queryId: string): boolean {
    const at = this.list.findIndex(x => x.approval.query_id === queryId);
    if (at < 0) return false;
    this.list.splice(at, 1);
    return true;
  }

  /// The card's approval, taken away as answered here.
  answer(): W | null {
    const w = this.list.shift() ?? null;
    if (w) this.answered.mark(w.approval.query_id, w.session);
    return w;
  }

  /// The answer to `w` failed: it is back on the card.
  failed(w: W) {
    this.answered.forget(w.approval.query_id);
    if (!this.list.some(x => x.approval.query_id === w.approval.query_id)) this.list.unshift(w);
  }

  /// Live mode ended: nothing waits here any more.
  clear() {
    this.list = [];
    this.answered = new AnsweredIds();
  }
}

/// An app as `list_apps` names it, as far as its logo goes.
export interface AppLogo {
  slug: string;
  name: string;
  logo_url: string | null;
}

/// The logo of the app an approval names, by name or slug, ignoring case.
/// Null when the app has none.
export function logoFor(apps: AppLogo[], app: string): string | null {
  const key = app.trim().toLowerCase().replace(/[\s_-]+/g, "");
  if (!key) return null;
  const norm = (s: string) => s.toLowerCase().replace(/[\s_-]+/g, "");
  const found = apps.find(a => norm(a.name) === key) ?? apps.find(a => norm(a.slug) === key);
  return found?.logo_url || null;
}
